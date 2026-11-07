import { decodeEventLog, parseUnits } from 'viem';
import { writeContract, waitForTransactionReceipt } from 'wagmi/actions';
import { assertServiceAvailable, ContractNotDeployedError } from './base';
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
    if (params.contractLanguage === 'lithic') {
      throw new Error('Native Lithic deployment is not available yet. No transaction was submitted.');
    }
    assertServiceAvailable(params.chainId);

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

    if (receipt.status !== 'success') {
      throw new Error('Token creation transaction reverted.');
    }
    const matchingAddresses: string[] = [];
    for (const log of receipt.logs) {
      if (log.address.toLowerCase() !== tokenFactory.toLowerCase()) continue;
      let decoded;
      try {
        decoded = decodeEventLog({
          abi: LITHO_TOKEN_FACTORY_ABI,
          data: log.data,
          topics: log.topics,
          strict: true,
        });
      } catch {
        continue;
      }
      if (decoded.eventName !== 'TokenCreated') continue;
      if (decoded.args.name !== params.name || decoded.args.symbol !== params.symbol) {
        throw new Error('TokenCreated metadata does not match the deployment request.');
      }
      const token = decoded.args.token;
      if (!/^0x[0-9a-fA-F]{40}$/.test(token) || /^0x0{40}$/i.test(token)) {
        throw new Error('TokenCreated contains an invalid token address.');
      }
      matchingAddresses.push(token);
    }
    if (matchingAddresses.length !== 1) {
      throw new Error('Expected exactly one TokenCreated event from the configured factory.');
    }
    const contractAddress = matchingAddresses[0];

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
