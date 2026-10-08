import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { type Page, expect } from '@playwright/test';
import { ENV_NAME, signatureCount, tokenBalance } from './rpc.ts';

export const dir = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', 'target', 'ui-e2e');
export const secret = JSON.parse(readFileSync(join(dir, 'wallet.json'), 'utf8')) as number[];

export interface Fixture {
  pool: string;
  hooked_mint: string;
  quote_mint: string;
  hook_program: string;
  wallet: string;
  hooked_account: string;
  quote_account: string;
}

/** What `ui-fixture` left for one hook on one AMM (see `e2e/global-setup.ts`). */
export const load = (hook: string, amm: string): Fixture =>
  JSON.parse(readFileSync(join(dir, `fixture-${hook}-${amm}.json`), 'utf8')) as Fixture;

/** The wallet's balance of the hooked or the quote token, read straight from the validator. */
export const balanceOf = (fixture: Fixture, mint: string): Promise<bigint> =>
  tokenBalance(mint === fixture.hooked_mint ? fixture.hooked_account : fixture.quote_account);

export const signaturesOf = (fixture: Fixture): Promise<number> => signatureCount(fixture.wallet);

/** Open the page on a fixture's pool with the test wallet connected, past the unknown-token gate. */
export async function openAndConnect(page: Page, fixture: Fixture, policyTestId = 'policy-phase'): Promise<void> {
  await page.goto(`/?env=${ENV_NAME}&pool=${fixture.pool}&testWallet=${encodeURIComponent(JSON.stringify(secret))}`);
  await page.getByRole('button', { name: /Connect E2E Test Wallet/ }).first().click();
  await expect(page.getByRole('button', { name: /Disconnect/ })).toBeVisible();
  // The pool, its policy and the unknown-token gate all appear together once the pool has loaded.
  await expect(page.getByTestId(policyTestId)).toBeVisible();
  const gate = page.getByRole('button', { name: 'I understand, continue' });
  while (await gate.first().isVisible().catch(() => false)) await gate.first().click();
}
