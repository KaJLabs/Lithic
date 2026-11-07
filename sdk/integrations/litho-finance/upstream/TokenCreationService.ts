import { decodeEventLog, parseUnits } from 'viem';
import { writeContract, waitForTransactionReceipt } from 'wagmi/actions';
import { assertServiceAvailable, ContractNotDeployedError, randomHash, randomAddress, simulateLatency } from './base';
import { getChainById } from '@/config/chains';
import { wagmiConfig } from '@/providers/wagmiConfig';
import { LITHO_TOKEN_FACTORY_ABI } from '@/config/abis/lithoTokenFactory';
import type { ActionContext, ActionResult } from '@/hooks/useContractAction';
import type { ChainConfig } from '@/types/chain';
import type { TokenFeatures } from '@/types/token';

export type ContractLanguage = 'lithic' | 'solidity';

export interface DeployTokenParams {
  chainId: ChainConfig['id'];
  name: string;
  symbol: string;
  totalSupply: string;
  decimals: number;
  features: TokenFeatures;
  /** Lithic compiles to native LithoVM bytecode; Solidity runs on Lithosphere's EVM layer. */
  contractLanguage: ContractLanguage;
}

/** Token factory integration layer. */
export const TokenCreationService = {
  async deployToken(params: DeployTokenParams, ctx: ActionContext): Promise<ActionResult> {
    assertServiceAvailable(params.chainId);

    if (params.contractLanguage === 'lithic') {
      // Lithic (native LithoVM) deployment isn't wired to a real compiler yet —
      // upstream Lithic still lacks collection-storage support needed for
      // token balances. Falls back to the demo path until that's viable.
      await simulateLatency(900, 1600);
      const hash = randomHash();
      ctx.onSubmitted(hash);
      return { hash, contractAddress: randomAddress() };
    }

    const chain = getChainById(params.chainId);
    const tokenFactory = chain?.contracts.tokenFactory;
    if (!tokenFactory) {
      throw new ContractNotDeployedError(`No token factory deployed on ${chain?.name ?? 'this network'} yet.`);
    }

    const numericChainId = Number(params.chainId);

    const hash = await writeContract(wagmiConfig, {
      address: tokenFactory as `0x${string}`,
      abi: LITHO_TOKEN_FACTORY_ABI,
      functionName: 'createToken',
      chainId: numericChainId as (typeof wagmiConfig)['chains'][number]['id'],
      args: [
        params.name,
        params.symbol,
        params.decimals,
        parseUnits(params.totalSupply, params.decimals),
        params.features.mintable,
        params.features.burnable,
        params.features.pausable,
        params.features.ownership,
      ],
    });

    ctx.onSubmitted(hash);

    const receipt = await waitForTransactionReceipt(wagmiConfig, {
      hash,
      chainId: numericChainId as (typeof wagmiConfig)['chains'][number]['id'],
    });

    let contractAddress: string | undefined;
    for (const log of receipt.logs) {
      try {
        const decoded = decodeEventLog({
          abi: LITHO_TOKEN_FACTORY_ABI,
          data: log.data,
          topics: log.topics,
        });
        if (decoded.eventName === 'TokenCreated') {
          contractAddress = decoded.args.token;
          break;
        }
      } catch {
        // Not the TokenCreated log (e.g. an ERC20 Transfer log from the same tx) — skip.
      }
    }

    return { hash, contractAddress };
  },

  estimateDeploymentCostUsd(features: TokenFeatures): number {
    let base = 8.5;
    if (features.mintable) base += 2.2;
    if (features.burnable) base += 1.8;
    if (features.pausable) base += 2.4;
    if (features.ownership) base += 1.4;
    return Math.round(base * 100) / 100;
  },
};
