import type { Message } from "@nomicfoundation/ethereumjs-evm";
import type { RunTxResult } from "@nomicfoundation/ethereumjs-vm";
import { Block } from "@nomicfoundation/ethereumjs-block";
import { Common } from "@nomicfoundation/ethereumjs-common";
import { TypedTransaction } from "@nomicfoundation/ethereumjs-tx";
import { Account, Address } from "@nomicfoundation/ethereumjs-util";

import { assertHardhatInvariant } from "../../../core/errors";
import { RpcDebugTracingConfig } from "../../../core/jsonrpc/types/input/debugTraceTransaction";
import { NodeConfig } from "../node-types";
import { RpcDebugTraceOutput } from "../output";
import { HardhatBlockchainInterface } from "../types/HardhatBlockchainInterface";

import { EthereumJSAdapter } from "./ethereumjs";
import { RethnetAdapter } from "./rethnet";
import { Trace, VMAdapter } from "./vm-adapter";

/* eslint-disable @nomiclabs/hardhat-internal-rules/only-hardhat-error */
/* eslint-disable @typescript-eslint/restrict-template-expressions */

function printEthereumJSTrace(trace: any) {
  console.log(JSON.stringify(trace, null, 2));
}

function printRethnetTrace(trace: any) {
  console.log(
    JSON.stringify(
      trace,
      (key, value) => (typeof value === "bigint" ? value.toString() : value),
      2
    )
  );
}

export class DualModeAdapter implements VMAdapter {
  constructor(
    private _ethereumJSAdapter: VMAdapter,
    private _rethnetAdapter: VMAdapter
  ) {}

  public static async create(
    common: Common,
    blockchain: HardhatBlockchainInterface,
    config: NodeConfig,
    selectHardfork: (blockNumber: bigint) => string
  ) {
    const ethereumJSAdapter = await EthereumJSAdapter.create(
      common,
      blockchain,
      config,
      selectHardfork
    );

    const rethnetAdapter = await RethnetAdapter.create(
      // eslint-disable-next-line @typescript-eslint/dot-notation
      ethereumJSAdapter["_stateManager"],
      config,
      selectHardfork,
      async (blockNumber) => {
        const block = await blockchain.getBlock(blockNumber);
        assertHardhatInvariant(
          block !== undefined && block !== null,
          "Should be able to get block"
        );

        return block.header.hash();
      }
    );

    return new DualModeAdapter(ethereumJSAdapter, rethnetAdapter);
  }

  public async dryRun(
    tx: TypedTransaction,
    blockContext: Block,
    forceBaseFeeZero?: boolean
  ): Promise<[RunTxResult, Trace]> {
    const [ethereumJSResult, ethereumJSTrace] =
      await this._ethereumJSAdapter.dryRun(tx, blockContext, forceBaseFeeZero);

    const [rethnetResult, rethnetTrace] = await this._rethnetAdapter.dryRun(
      tx,
      blockContext,
      forceBaseFeeZero
    );

    try {
      assertEqualRunTxResults(ethereumJSResult, rethnetResult);
      return [rethnetResult, rethnetTrace];
    } catch (e) {
      // if the results didn't match, print the traces
      console.log("EthereumJS trace");
      printEthereumJSTrace(ethereumJSTrace);
      console.log();
      console.log("Rethnet trace");
      printRethnetTrace(rethnetTrace);

      throw e;
    }
  }

  public async getStateRoot(): Promise<Buffer> {
    const ethereumJSRoot = await this._ethereumJSAdapter.getStateRoot();
    const rethnetRoot = await this._rethnetAdapter.getStateRoot();

    if (!ethereumJSRoot.equals(rethnetRoot)) {
      console.trace(
        `Different state root: ${ethereumJSRoot.toString(
          "hex"
        )} !== ${rethnetRoot.toString("hex")}`
      );
      throw new Error("Different state root");
    }

    return rethnetRoot;
  }

  public async getAccount(address: Address): Promise<Account> {
    const ethereumJSAccount = await this._ethereumJSAdapter.getAccount(address);
    const rethnetAccount = await this._rethnetAdapter.getAccount(address);

    assertEqualAccounts(ethereumJSAccount, rethnetAccount);

    return ethereumJSAccount;
  }

  public async getContractStorage(
    address: Address,
    key: Buffer
  ): Promise<Buffer> {
    const ethereumJSStorageSlot =
      await this._ethereumJSAdapter.getContractStorage(address, key);

    const rethnetStorageSlot = await this._rethnetAdapter.getContractStorage(
      address,
      key
    );

    if (!ethereumJSStorageSlot.equals(rethnetStorageSlot)) {
      console.trace(
        `Different storage slot: ${ethereumJSStorageSlot} !== ${rethnetStorageSlot}`
      );
      throw new Error("Different storage slot");
    }

    return rethnetStorageSlot;
  }

  public async getContractCode(address: Address): Promise<Buffer> {
    const ethereumJSCode = await this._ethereumJSAdapter.getContractCode(
      address
    );
    const rethnetCode = await this._rethnetAdapter.getContractCode(address);

    if (!ethereumJSCode.equals(rethnetCode)) {
      console.trace(
        `Different contract code: ${ethereumJSCode} !== ${rethnetCode}`
      );
      throw new Error("Different contract code");
    }

    return rethnetCode;
  }

  public async putAccount(address: Address, account: Account): Promise<void> {
    await this._rethnetAdapter.putAccount(address, account);
    return this._ethereumJSAdapter.putAccount(address, account);
  }

  public async putContractCode(address: Address, value: Buffer): Promise<void> {
    return this._ethereumJSAdapter.putContractCode(address, value);
  }

  public async putContractStorage(
    address: Address,
    key: Buffer,
    value: Buffer
  ): Promise<void> {
    await this._rethnetAdapter.putContractStorage(address, key, value);
    return this._ethereumJSAdapter.putContractStorage(address, key, value);
  }

  public async restoreContext(stateRoot: Buffer): Promise<void> {
    await this._ethereumJSAdapter.restoreContext(stateRoot);
    return this._rethnetAdapter.restoreContext(stateRoot);
  }

  public async traceTransaction(
    hash: Buffer,
    block: Block,
    config: RpcDebugTracingConfig
  ): Promise<RpcDebugTraceOutput> {
    return this._ethereumJSAdapter.traceTransaction(hash, block, config);
  }

  public enableTracing(callbacks: {
    beforeMessage: (message: Message, next: any) => Promise<void>;
    step: () => Promise<void>;
    afterMessage: () => Promise<void>;
  }): void {
    return this._ethereumJSAdapter.enableTracing(callbacks);
  }

  public disableTracing(): void {
    return this._ethereumJSAdapter.disableTracing();
  }

  public async setBlockContext(
    block: Block,
    irregularStateOrUndefined: Buffer | undefined
  ): Promise<void> {
    return this._ethereumJSAdapter.setBlockContext(
      block,
      irregularStateOrUndefined
    );
  }

  public async startBlock(): Promise<void> {
    await this._rethnetAdapter.startBlock();
    return this._ethereumJSAdapter.startBlock();
  }

  public async runTxInBlock(
    tx: TypedTransaction,
    block: Block
  ): Promise<[RunTxResult, Trace]> {
    const [ethereumJSResult, ethereumJSTrace] =
      await this._ethereumJSAdapter.runTxInBlock(tx, block);

    const [rethnetResult, rethnetTrace] =
      await this._rethnetAdapter.runTxInBlock(tx, block);

    try {
      assertEqualRunTxResults(ethereumJSResult, rethnetResult);
      return [ethereumJSResult, ethereumJSTrace];
    } catch (e) {
      // if the results didn't match, print the traces
      console.log("EthereumJS trace");
      printEthereumJSTrace(ethereumJSTrace);
      console.log();
      console.log("Rethnet trace");
      printRethnetTrace(rethnetTrace);

      throw e;
    }
  }

  public async addBlockRewards(
    rewards: Array<[Address, bigint]>
  ): Promise<void> {
    await this._rethnetAdapter.addBlockRewards(rewards);
    return this._ethereumJSAdapter.addBlockRewards(rewards);
  }

  public async sealBlock(): Promise<void> {
    await this._rethnetAdapter.sealBlock();
    return this._ethereumJSAdapter.sealBlock();
  }

  public async revertBlock(): Promise<void> {
    await this._rethnetAdapter.revertBlock();
    return this._ethereumJSAdapter.revertBlock();
  }
}

function assertEqualRunTxResults(
  ethereumJSResult: RunTxResult,
  rethnetResult: RunTxResult
) {
  if (ethereumJSResult.totalGasSpent !== rethnetResult.totalGasSpent) {
    console.trace(
      `Different totalGasSpent: ${ethereumJSResult.totalGasSpent} !== ${rethnetResult.totalGasSpent}`
    );
    throw new Error("Different totalGasSpent");
  }
  if (ethereumJSResult.gasRefund !== rethnetResult.gasRefund) {
    console.trace(
      `Different gasRefund: ${ethereumJSResult.gasRefund} !== ${rethnetResult.gasRefund}`
    );
    throw new Error("Different gasRefund");
  }
  if (
    ethereumJSResult.createdAddress?.toString() !==
    rethnetResult.createdAddress?.toString()
  ) {
    console.trace(
      `Different createdAddress: ${ethereumJSResult.createdAddress?.toString()} !== ${rethnetResult.createdAddress?.toString()}`
    );
    throw new Error("Different createdAddress");
  }

  if (
    ethereumJSResult.execResult.exceptionError?.error !==
    rethnetResult.execResult.exceptionError?.error
  ) {
    console.trace(
      `Different exceptionError.error: ${ethereumJSResult.execResult.exceptionError?.error} !== ${rethnetResult.execResult.exceptionError?.error}`
    );
    throw new Error("Different exceptionError.error");
  }

  if (
    ethereumJSResult.execResult.exceptionError?.errorType !==
    rethnetResult.execResult.exceptionError?.errorType
  ) {
    console.trace(
      `Different exceptionError.errorType: ${ethereumJSResult.execResult.exceptionError?.errorType} !== ${rethnetResult.execResult.exceptionError?.errorType}`
    );
    throw new Error("Different exceptionError.errorType");
  }

  // TODO: we only compare the return values when a contract was *not* created,
  // because sometimes ethereumjs has the created bytecode in the return value
  // and rethnet doesn't
  if (ethereumJSResult.createdAddress === undefined) {
    if (
      ethereumJSResult.execResult.returnValue.toString("hex") !==
      rethnetResult.execResult.returnValue.toString("hex")
    ) {
      console.trace(
        `Different returnValue: ${ethereumJSResult.execResult.returnValue.toString(
          "hex"
        )} !== ${rethnetResult.execResult.returnValue.toString("hex")}`
      );
      throw new Error("Different returnValue");
    }
  }
}

function assertEqualAccounts(
  ethereumJSAccount: Account,
  rethnetAccount: Account
) {
  if (ethereumJSAccount.balance !== rethnetAccount.balance) {
    console.trace(
      `Different balance: ${ethereumJSAccount.balance} !== ${rethnetAccount.balance}`
    );
    throw new Error("Different balance");
  }

  if (!ethereumJSAccount.codeHash.equals(rethnetAccount.codeHash)) {
    console.trace(
      `Different codeHash: ${ethereumJSAccount.codeHash} !== ${rethnetAccount.codeHash}`
    );
    throw new Error("Different codeHash");
  }

  if (ethereumJSAccount.nonce !== rethnetAccount.nonce) {
    console.trace(
      `Different nonce: ${ethereumJSAccount.nonce} !== ${rethnetAccount.nonce}`
    );
    throw new Error("Different nonce");
  }

  // TODO: Add storageRoot to Rethnet
  // if (ethereumJSAccount.storageRoot !== rethnetAccount.storageRoot) {
  //   console.trace(
  //     `Different storageRoot: ${ethereumJSAccount.storageRoot} !== ${rethnetAccount.storageRoot}`
  //   );
  //   throw new Error("Different storageRoot");
  // }
}