import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, type Page } from '@playwright/test';
import { DEVNET, ENV_NAME, signatureCount, tokenBalance } from './rpc.ts';

const dir = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', 'target', 'ui-e2e');
const secret = JSON.parse(readFileSync(join(dir, 'wallet.json'), 'utf8')) as number[];

interface Fixture {
  pool: string;
  hooked_mint: string;
  quote_mint: string;
  hook_program: string;
  wallet: string;
  hooked_account: string;
  quote_account: string;
}

const load = (amm: string): Fixture => JSON.parse(readFileSync(join(dir, `fixture-${amm}.json`), 'utf8')) as Fixture;

async function openAndConnect(page: Page, fixture: Fixture): Promise<void> {
  await page.goto(`/?env=${ENV_NAME}&pool=${fixture.pool}&testWallet=${encodeURIComponent(JSON.stringify(secret))}`);
  await page.getByRole('button', { name: /Connect E2E Test Wallet/ }).first().click();
  await expect(page.getByRole('button', { name: /Disconnect/ })).toBeVisible();
  // The pool, its policy and the unknown-token gate all appear together once the pool has loaded.
  await expect(page.getByTestId('policy-phase')).toContainText('Active');
  const gate = page.getByRole('button', { name: 'I understand, continue' });
  while (await gate.first().isVisible().catch(() => false)) await gate.first().click();
}

// The same scenario against a CPMM pool and a CLMM pool: the page picks the adapter from the pool's owner.
const scenarios = [
  ['cpmm', 'CPMM  swap_base_input_v2'],
  ['clmm', 'CLMM  swap_v3'],
] as const;
const enabled = (process.env.E2E_AMMS ?? 'cpmm,clmm').split(',');
for (const [amm, instruction] of scenarios.filter(([name]) => !DEVNET || enabled.includes(name))) {
  test.describe(`${amm} Fair Launch pool`, () => {
    test.describe.configure({ mode: 'serial' });
    const fixture = load(amm);
    const balance = (mint: string): Promise<bigint> =>
      tokenBalance(mint === fixture.hooked_mint ? fixture.hooked_account : fixture.quote_account);
    const signatures = (): Promise<number> => signatureCount(fixture.wallet);

    test('says it is experimental and loads the pool and its policy', async ({ page }) => {
      await openAndConnect(page, fixture);
      await expect(page.getByText('Experimental Raydium Transfer Hook environment')).toBeVisible();
      await expect(page.getByText('Not an official Raydium deployment')).toBeVisible();
      await expect(page.getByText('Fair Launch policy')).toBeVisible();
    });

    test(`a buy inside the limits succeeds through ${instruction.split('  ')[1]}`, async ({ page }) => {
      await openAndConnect(page, fixture);
      const before = await balance(fixture.hooked_mint);
      await page.getByRole('button', { name: 'Switch direction' }).click();
      await page.getByLabel('From').fill('10');
      await expect(page.getByText('BUY PROTECTIONS ACTIVE')).toBeVisible();
      await expect(page.getByTestId('preflight-warning')).toHaveCount(0);
      await page.getByRole('button', { name: 'Swap', exact: true }).click();
      await expect(page.getByText('Swap confirmed')).toBeVisible();
      expect(await balance(fixture.hooked_mint)).toBeGreaterThan(before);
      await page.getByText('Developer details').click();
      await expect(page.getByTestId('developer-details')).toContainText(instruction);
      await expect(page.getByTestId('developer-details')).toContainText('Input hook accounts (0)');
    });

    test('an over-limit buy is refused by the hook, shown in words, and never submitted', async ({ page }) => {
      await openAndConnect(page, fixture);
      const hookedBefore = await balance(fixture.hooked_mint);
      const quoteBefore = await balance(fixture.quote_mint);
      const signaturesBefore = await signatures();
      await page.getByRole('button', { name: 'Switch direction' }).click();
      await page.getByLabel('From').fill('150');
      await expect(page.getByTestId('preflight-warning')).toContainText('Buy amount');
      await page.getByRole('button', { name: 'Swap', exact: true }).click();
      const alert = page.getByRole('alert');
      await expect(alert).toContainText('Swap blocked by Fair Launch');
      await expect(page.getByTestId('failure-reason')).toContainText('larger than the launch max-buy limit');
      await expect(alert).toContainText('No transaction was submitted.');
      expect(await balance(fixture.hooked_mint)).toBe(hookedBefore);
      expect(await balance(fixture.quote_mint)).toBe(quoteBefore);
      expect(await signatures()).toBe(signaturesBefore);
    });

    test('a sell succeeds and the page does not apply buy rules to it', async ({ page }) => {
      await openAndConnect(page, fixture);
      const before = await balance(fixture.quote_mint);
      await page.getByLabel('From').fill('5');
      await expect(page.getByText('Fair Launch buy restrictions do not apply to this sell.')).toBeVisible();
      await page.getByRole('button', { name: 'Swap', exact: true }).click();
      await expect(page.getByText('Swap confirmed')).toBeVisible();
      expect(await balance(fixture.quote_mint)).toBeGreaterThan(before);
    });
  });
}

test('the layout fits a phone without horizontal scrolling', async ({ page }) => {
  // A desktop picture first, for the docs: a typed buy shows the quote, the policy meters and the hook row.
  await page.setViewportSize({ width: 1280, height: 900 });
  await openAndConnect(page, load('cpmm'));
  await page.getByRole('button', { name: 'Switch direction' }).click();
  await page.getByLabel('From').fill('10');
  await expect(page.getByText('BUY PROTECTIONS ACTIVE')).toBeVisible();
  await page.screenshot({ path: join(dir, 'desktop.png') });

  await page.setViewportSize({ width: 390, height: 844 });
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  expect(overflow).toBeLessThanOrEqual(0);
  await page.screenshot({ path: join(dir, 'mobile.png'), fullPage: true });
});
