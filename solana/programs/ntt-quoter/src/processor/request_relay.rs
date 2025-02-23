use anchor_lang::{
    prelude::*,
    system_program::{self, Transfer},
};
use example_native_token_transfers::queue::outbox::OutboxItem;
use solana_program::{native_token::LAMPORTS_PER_SOL, sysvar};

use crate::{
    error::NttQuoterError,
    state::{Instance, RegisteredChain, RelayRequest},
    EVM_GAS_COST, WORMHOLE_TRANSCEIVER_INDEX,
};

#[derive(Accounts)]
pub struct RequestRelay<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub instance: Account<'info, Instance>,

    #[account(
        seeds = [
            RegisteredChain::SEED_PREFIX,
            outbox_item.recipient_chain.id.to_be_bytes().as_ref()
        ],
        bump = registered_chain.bump,
        constraint = registered_chain.base_price != u64::MAX @
            NttQuoterError::RelayingToChainDisabled
    )]
    pub registered_chain: Account<'info, RegisteredChain>,

    //TODO eventually drop the released constraint and instead implement release by relayer
    #[account(
        owner = example_native_token_transfers::ID,
        constraint = outbox_item.released.get(WORMHOLE_TRANSCEIVER_INDEX),
    )]
    pub outbox_item: Account<'info, OutboxItem>,

    #[account(
        init,
        payer = payer,
        space = 8 + RelayRequest::INIT_SPACE,
        seeds = [RelayRequest::SEED_PREFIX, outbox_item.key().as_ref()],
        bump,
    )]
    pub relay_request: Account<'info, RelayRequest>,

    pub system_program: Program<'info, System>,
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct RequestRelayArgs {
    pub gas_dropoff: u64, //NativeAmount,
    pub max_fee: u64,     //SolAmount,
}

const GWEI: u64 = u64::pow(10, 9);

//TODO built-in u128 division likely still wastes a ton of compute units
//     might be more efficient to use f64 or ruint crate
fn mul_div(scalar: u64, numerator: u64, denominator: u64) -> u64 {
    if scalar > 0 {
        //avoid potentially expensive u128 division
        ((scalar as u128) * (numerator as u128) / (denominator as u128))
            .try_into()
            .unwrap()
    } else {
        0
    }
}

pub fn request_relay(ctx: Context<RequestRelay>, args: RequestRelayArgs) -> Result<()> {
    let accs = ctx.accounts;

    require_gte!(
        accs.registered_chain.max_gas_dropoff,
        args.gas_dropoff,
        NttQuoterError::ExceedsMaxGasDropoff
    );

    let relay_fee_in_lamports = {
        let target_native_in_gwei =
            args.gas_dropoff + mul_div(accs.registered_chain.gas_price, EVM_GAS_COST, GWEI);

        //usd/target_native[usd, 6 decimals] * target_native[gwei, 9 decimals] = usd[usd, 6 decimals]
        let target_native_in_usd = mul_div(
            accs.registered_chain.native_price,
            target_native_in_gwei,
            GWEI,
        );

        let total_in_usd = target_native_in_usd + accs.registered_chain.base_price;

        //total_fee[sol, 9 decimals] = total_usd[usd, 6 decimals] / (sol_price[usd, 6 decimals]
        mul_div(total_in_usd, LAMPORTS_PER_SOL, accs.instance.sol_price)
    };

    let rent_in_lamports = sysvar::rent::Rent::get()?.minimum_balance(8 + RelayRequest::INIT_SPACE);
    let fee_minus_rent = relay_fee_in_lamports.saturating_sub(rent_in_lamports);
    let total_fee_in_lamports = fee_minus_rent + rent_in_lamports;

    require_gte!(
        args.max_fee,
        total_fee_in_lamports,
        NttQuoterError::ExceedsUserMaxFee
    );

    msg!("total fee in lamports: {}", total_fee_in_lamports);

    //store the requested gas dropoff
    accs.relay_request.requested_gas_dropoff = args.gas_dropoff;

    //pay the relayer by adding fee on top of account rent (if any)
    if fee_minus_rent > 0 {
        system_program::transfer(
            CpiContext::new(
                accs.system_program.to_account_info(),
                Transfer {
                    from: accs.payer.to_account_info(),
                    to: accs.relay_request.to_account_info(),
                },
            ),
            fee_minus_rent,
        )?;
    }

    Ok(())
}
