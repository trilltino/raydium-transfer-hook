type TransferContext = {
  source: PublicKey;
  mint: PublicKey;
  destination: PublicKey;
  authority: PublicKey;
};

async function resolveTransferAccounts(
  context: TransferContext,
): Promise<AccountMeta[]> {
  const mintState = await fetchAndValidateMint(context.mint);
  const hookProgram = readTransferHookProgram(mintState);
  if (hookProgram === null) return [];

  const validationList = await fetchAndValidateExtraAccountMetaList(
    hookProgram,
    context.mint,
  );

  return resolveExtraAccountMetas(validationList, {
    ...context,
    hookProgram,
  });
}

async function buildHookAwareInstruction(
  transfers: TransferContext[],
): Promise<Instruction> {
  const perTransfer = await Promise.all(
    transfers.map(resolveTransferAccounts),
  );

  // Keep one ordered range per transfer until the target instruction defines
  // an explicit account-slice contract. A global union is not safe by default.
  const remainingAccounts = perTransfer.flat();
  return buildRaydiumInstructionWithTransferRanges(remainingAccounts, perTransfer);
}

declare type PublicKey = unknown;
declare type AccountMeta = unknown;
declare type Instruction = unknown;
declare function fetchAndValidateMint(mint: PublicKey): Promise<unknown>;
declare function readTransferHookProgram(mint: unknown): PublicKey | null;
declare function fetchAndValidateExtraAccountMetaList(
  hookProgram: PublicKey,
  mint: PublicKey,
): Promise<unknown>;
declare function resolveExtraAccountMetas(
  validationList: unknown,
  context: TransferContext & { hookProgram: PublicKey },
): Promise<AccountMeta[]>;
declare function buildRaydiumInstructionWithTransferRanges(
  remainingAccounts: AccountMeta[],
  transferRanges: AccountMeta[][],
): Instruction;
