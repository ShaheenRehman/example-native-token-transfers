import { inspect } from "util";
import { ChainId } from "@certusone/wormhole-sdk";
import {
    WormholeTransceiver__factory,
    NttManager__factory,
    ISpecialRelayer__factory,
} from "../contract-bindings";
import {
  loadOperatingChains,
  init,
  ChainInfo,
  getSigner,
  getContractAddress,
  loadScriptConfig,
} from "./env";

const processName = "readTransceiverConfig";

type Peer = {
  chainId: ChainId;
};

init();
const chains = loadOperatingChains();
async function run() {
  // Warning: we assume that the script configuration file is correctly formed
  console.log(`Start ${processName}!`);
  const peers = await loadScriptConfig("peers") as Peer[];

  const results = await Promise.all(
    chains.map(async chain => {
      try {
        await readContractConfig(chain, peers);
      } catch (error) {
        return { chainId: chain.chainId, error };
      }
      
      return { chainId: chain.chainId };
    })
  )

  for (const result of results) {
    if (("error" in result)) {
      console.error(
        `Error configuring contract for chain ${result.chainId}: ${inspect(
          result.error
        )}`
      );
      continue;
    }

    console.log(`Configuration succeded for chain ${result.chainId}`);
  }
}
async function readContractConfig(chain: ChainInfo, peers: Peer[]) {
  const log = (...args: any[]) => console.log(`[${chain.chainId}]`, ...args);

  const transceiverContract = await getTransceiverContract(chain);
  const managerContract = await getManagerContract(chain);
  const specialRelayer = await getSpecializedRelayer(chain);

  const tokenContract = await managerContract.token();

  const result = await specialRelayer.quoteDeliveryPrice(tokenContract, 1, 0);

  log("Reading contract configuration...");
  log("Token address: ", await transceiverContract.getNttManagerToken());
  log("Peers information:");
  for (const peer of peers) {
    log("Peer:", peer.chainId);
    log("  Specialized relaying enabled:", await transceiverContract.isSpecialRelayingEnabled(peer.chainId));
    log("  Wormhole relaying enabled:", await transceiverContract.isWormholeRelayingEnabled(peer.chainId));
    log("  Wormhole evm chain:", await transceiverContract.isWormholeEvmChain(peer.chainId));
  }
  
  return { chainId: chain.chainId };
}

async function getTransceiverContract(chain: ChainInfo) {
  const signer = await getSigner(chain);
  const transceiverAddress = await getContractAddress("NttTransceiverProxies", chain.chainId);
  return WormholeTransceiver__factory.connect(transceiverAddress, signer);
}

async function getManagerContract(chain: ChainInfo) {
  const signer = await getSigner(chain);
  const managerAddress = await getContractAddress("NttManagerProxies", chain.chainId);
  return NttManager__factory.connect(managerAddress, signer);
}

async function getSpecializedRelayer(chain: ChainInfo) {
  const signer = await getSigner(chain);
  const specializedRelayerAddress = await getContractAddress("SpecializedRelayers", chain.chainId);
  return ISpecialRelayer__factory.connect(specializedRelayerAddress, signer);
}

run().then(() => console.log("Done!"));
