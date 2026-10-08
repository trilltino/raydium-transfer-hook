import { join } from 'node:path';
import { expect, test } from '@playwright/test';
import { balanceOf, dir, load, openAndConnect, secret, signaturesOf } from './helpers.ts';
import { DEVNET, ENV_NAME, airdrop, newWallet, ownedBalance } from './rpc.ts';

// The same scenario against a CPMM pool and a CLMM pool: the page picks the adapter from the pool's owner.
const scenarios = [
  ['cpmm', 'CPMM  swap_base_input_v2'],
  ['clmm', 'CLMM  swap_v3'],
] as const;
const enabled = (process.env.E2E_AMMS ?? 'cpmm,clmm').split(',');

for (const [amm, instruction] of scenarios.filter(([name]) => !DEVNET || enabled.includes(name))) {
  test.describe(`${amm} Fair Launch pool`, () => {
    test.describe.configure({ mode: 'serial' });
    const fixture = load('fair-launch', amm);

    const open = (page: Parameters<typeof openAndConnect>[0]) => openAndConnect(page, fixture);

    test('loads the pool and its policy', async ({ page }) => {
      await open(page);
      await expect(page.getByTestId('policy-phase')).toContainText('Active');
      await expect(page.getByText('Fair Launch policy')).toBeVisible();
    });

    test(`a buy inside the limits succeeds through ${instruction.split('  ')[1]}`, async ({ page }) => {
      await open(page);
      const before = await balanceOf(fixture, fixture.hooked_mint);
      await page.getByRole('button', { name: 'Switch direction' }).click();
      await page.getByLabel('From').fill('10');
      await expect(page.getByText('BUY PROTECTIONS ACTIVE')).toBeVisible();
      await expect(page.getByTestId('preflight-warning')).toHaveCount(0);
      await page.getByRole('button', { name: 'Swap', exact: true }).click();
      await expect(page.getByText('Swap confirmed')).toBeVisible();
      expect(await balanceOf(fixture, fixture.hooked_mint)).toBeGreaterThan(before);
      await page.getByText('Developer details').click();
      await expect(page.getByTestId('developer-details')).toContainText(instruction);
      await expect(page.getByTestId('developer-details')).toContainText('Input hook accounts (0)');
    });

    test('a confirmed swap is traced step by step with Solscan links', async ({ page }) => {
      await open(page);
      await page.getByRole('button', { name: 'Switch direction' }).click();
      await page.getByLabel('From').fill('9');
      await page.getByRole('button', { name: 'Swap', exact: true }).click();
      await expect(page.getByText('Swap confirmed')).toBeVisible();
      // Every landed transaction links to Solscan.
      await expect(page.getByTestId('solscan-tx')).toHaveAttribute('href', /^https:\/\/solscan\.io\/tx\/[1-9A-HJ-NP-Za-km-z]{60,}/);
      // The page reads the landed transaction back (devnet through our Triton One endpoint, a local validator from
      // its own RPC) and shows the programs in the order they ran: the swap, each Token-2022 transfer, the hook.
      await expect(page.getByTestId('trace-summary')).toBeVisible({ timeout: 90_000 });
      await expect(page.getByTestId('trace-summary')).toContainText(DEVNET ? 'Triton One' : 'local validator');
      await expect(page.getByTestId('trace-hooks')).toContainText('ran 1 time');
      expect(await page.getByTestId('trace-step').count()).toBeGreaterThanOrEqual(4);
      await expect(page.getByTestId('trace-step').filter({ hasText: instruction.split('  ')[1] })).toHaveCount(1);
      await expect(page.getByTestId('trace-step').filter({ hasText: 'Execute (Transfer Hook)' })).toHaveCount(1);
      await expect(page.getByRole('link', { name: 'program ↗' }).first()).toHaveAttribute(
        'href',
        DEVNET ? /solscan\.io\/account\/.*cluster=devnet/ : /solscan\.io\/account\/.*cluster=custom/
      );
      await page.getByTestId('trace').screenshot({ path: join(dir, DEVNET ? 'trace-devnet.png' : 'trace-local.png') });
    });

    test('a wallet with no tokens sees why, gets some from the faucet, and can then swap', async ({ page }) => {
      test.skip(DEVNET, 'the devnet faucet needs the deployer key; the local one needs nothing');
      const empty = newWallet();
      await page.goto(`/?env=${ENV_NAME}&pool=${fixture.pool}&testWallet=${encodeURIComponent(JSON.stringify(empty.secret))}`);
      await page.getByRole('button', { name: /Connect E2E Test Wallet/ }).first().click();
      await expect(page.getByTestId('policy-phase')).toBeVisible();
      const gate = page.getByRole('button', { name: 'I understand, continue' });
      while (await gate.first().isVisible().catch(() => false)) await gate.first().click();
      // Before anything is clicked, Developer details says what the zero balances mean and what to do.
      await page.getByText('Developer details').click();
      await expect(page.getByTestId('developer-details')).toContainText('holds none of this pool’s tokens');
      await expect(page.getByTestId('developer-details')).toContainText(`run fund -- ${empty.address}`);
      await page.getByRole('button', { name: 'Get test tokens' }).click();
      await expect(page.getByTestId('test-tokens-status')).toContainText('Minted 100 of 2 tokens');
      expect(await ownedBalance(empty.address, fixture.hooked_mint)).toBe(100_000_000n);
      await expect(page.getByTestId('developer-details')).not.toContainText('holds none of this pool’s tokens');
      // Tokens but no SOL: the page says that in words, not "AccountNotFound", and nothing is submitted.
      await page.getByRole('button', { name: 'Switch direction' }).click();
      await page.getByLabel('From').fill('10');
      await page.getByRole('button', { name: 'Swap', exact: true }).click();
      await expect(page.getByRole('alert')).toContainText('Your wallet has no SOL on this network');
      await expect(page.getByRole('alert')).toContainText('No transaction was submitted.');
      // With SOL the same swap goes through.
      await airdrop(empty.address, 1_000_000_000);
      await page.getByLabel('From').fill('11');
      await page.getByRole('button', { name: 'Swap', exact: true }).click();
      await expect(page.getByText('Swap confirmed')).toBeVisible();
    });

    test('the panel says what the launch enforces, and its try-it buttons show the hook refusing', async ({ page }) => {
      await open(page);
      const rules = page.getByTestId('policy-rules');
      await expect(rules).toContainText('No single buy may take more than');
      await expect(rules).toContainText('a bundle with more is refused as a whole');
      await expect(rules).toContainText('Selling is never restricted');
      await page.getByRole('button', { name: 'A buy inside the limits' }).click();
      await expect(page.getByText('BUY PROTECTIONS ACTIVE')).toBeVisible();
      await expect(page.getByTestId('preflight-warning')).toHaveCount(0);
      await page.getByRole('button', { name: 'A buy over the limit' }).click();
      await expect(page.getByTestId('preflight-warning')).toContainText('Buy amount');
      await page.getByRole('button', { name: 'Swap', exact: true }).click();
      await expect(page.getByTestId('failure-reason')).toContainText('larger than the launch max-buy limit');
    });

    test('an over-limit buy is refused by the hook, shown in words, and never submitted', async ({ page }) => {
      await open(page);
      const hookedBefore = await balanceOf(fixture, fixture.hooked_mint);
      const quoteBefore = await balanceOf(fixture, fixture.quote_mint);
      const signaturesBefore = await signaturesOf(fixture);
      await page.getByRole('button', { name: 'Switch direction' }).click();
      await page.getByLabel('From').fill('150');
      await expect(page.getByTestId('preflight-warning')).toContainText('Buy amount');
      await page.getByRole('button', { name: 'Swap', exact: true }).click();
      const alert = page.getByRole('alert');
      await expect(alert).toContainText('Swap blocked by Fair Launch');
      await expect(page.getByTestId('failure-reason')).toContainText('larger than the launch max-buy limit');
      await expect(alert).toContainText('No transaction was submitted.');
      expect(await balanceOf(fixture, fixture.hooked_mint)).toBe(hookedBefore);
      expect(await balanceOf(fixture, fixture.quote_mint)).toBe(quoteBefore);
      expect(await signaturesOf(fixture)).toBe(signaturesBefore);
    });

    test('a sell succeeds and the page does not apply buy rules to it', async ({ page }) => {
      await open(page);
      const before = await balanceOf(fixture, fixture.quote_mint);
      await page.getByLabel('From').fill('5');
      await expect(page.getByText('Fair Launch buy restrictions do not apply to this sell.')).toBeVisible();
      await page.getByRole('button', { name: 'Swap', exact: true }).click();
      await expect(page.getByText('Swap confirmed')).toBeVisible();
      expect(await balanceOf(fixture, fixture.quote_mint)).toBeGreaterThan(before);
    });
  });
}

test('the layout fits a phone without horizontal scrolling', async ({ page }) => {
  // A desktop picture first, for the docs: a typed buy shows the quote, the policy meters and the hook row.
  await page.setViewportSize({ width: 1280, height: 900 });
  await openAndConnect(page, load('fair-launch', 'cpmm'));
  await page.getByRole('button', { name: 'Switch direction' }).click();
  await page.getByLabel('From').fill('10');
  await expect(page.getByText('BUY PROTECTIONS ACTIVE')).toBeVisible();
  await page.screenshot({ path: join(dir, 'desktop.png') });

  await page.setViewportSize({ width: 390, height: 844 });
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  expect(overflow).toBeLessThanOrEqual(0);
  await page.screenshot({ path: join(dir, 'mobile.png'), fullPage: true });
  void secret;
});
