import { expect, test } from '@playwright/test';
import { balanceOf, load, openAndConnect, signaturesOf } from './helpers.ts';
import { DEVNET } from './rpc.ts';

// The same trading shell with a different policy panel: a creator's allocation that vests. The wallet
// is the creator account: 100 tokens, of which 80 are locked until the schedule runs out.
test.skip(DEVNET, 'the creator-commitment pool is only built for the local validator');

test.describe('Creator Commitment pool (CPMM)', () => {
  test.describe.configure({ mode: 'serial' });
  const fixture = load('creator-commitment', 'cpmm');
  const open = (page: Parameters<typeof openAndConnect>[0]) => openAndConnect(page, fixture);

  test('shows the vesting schedule and that the wallet is the creator account', async ({ page }) => {
    await open(page);
    await expect(page.getByText('Creator commitment', { exact: true })).toBeVisible();
    await expect(page.getByTestId('policy-phase')).toContainText('Locked until the cliff');
    await expect(page.getByTestId('locked-total')).toHaveText('80');
    await expect(page.getByTestId('locked-now')).toHaveText('80');
    await expect(page.getByTestId('creator-account')).toHaveText(fixture.hooked_account);
    await expect(page.getByText('YOUR WALLET IS THE CREATOR ACCOUNT')).toBeVisible();
  });

  test('a sale below the vesting floor is refused by the hook, shown in words, and never submitted', async ({ page }) => {
    await open(page);
    const hookedBefore = await balanceOf(fixture, fixture.hooked_mint);
    const quoteBefore = await balanceOf(fixture, fixture.quote_mint);
    const signaturesBefore = await signaturesOf(fixture);
    // 100 held and 80 locked: selling 30 would leave 70
    await page.getByLabel('From').fill('30');
    await expect(page.getByTestId('preflight-warning')).toContainText('vesting floor');
    await page.getByRole('button', { name: 'Swap', exact: true }).click();
    const alert = page.getByRole('alert');
    await expect(alert).toContainText('Blocked by the creator’s vesting schedule');
    await expect(page.getByTestId('failure-reason')).toContainText('below the amount still locked');
    await expect(alert).toContainText('No transaction was submitted.');
    expect(await balanceOf(fixture, fixture.hooked_mint)).toBe(hookedBefore);
    expect(await balanceOf(fixture, fixture.quote_mint)).toBe(quoteBefore);
    expect(await signaturesOf(fixture)).toBe(signaturesBefore);
  });

  test('a sale that leaves the floor intact goes through', async ({ page }) => {
    await open(page);
    const before = await balanceOf(fixture, fixture.hooked_mint);
    await page.getByLabel('From').fill('15'); // leaves 85, above the 80 locked
    await expect(page.getByTestId('floor-check')).toHaveAttribute('data-violated', 'false');
    await page.getByRole('button', { name: 'Swap', exact: true }).click();
    await expect(page.getByText('Swap confirmed')).toBeVisible();
    expect(await balanceOf(fixture, fixture.hooked_mint)).toBe(before - 15_000_000n);
    await page.getByText('Developer details').click();
    await expect(page.getByTestId('developer-details')).toContainText('CPMM  swap_base_input_v2');
  });

  test('buying is never restricted, and the creator account keeps what it receives', async ({ page }) => {
    await open(page);
    const before = await balanceOf(fixture, fixture.hooked_mint);
    await page.getByRole('button', { name: 'Switch direction' }).click();
    await page.getByLabel('From').fill('10');
    await page.getByRole('button', { name: 'Swap', exact: true }).click();
    await expect(page.getByText('Swap confirmed')).toBeVisible();
    expect(await balanceOf(fixture, fixture.hooked_mint)).toBeGreaterThan(before);
  });
});
