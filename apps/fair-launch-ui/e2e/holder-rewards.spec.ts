import { expect, test } from '@playwright/test';
import { balanceOf, load, openAndConnect } from './helpers.ts';
import { DEVNET } from './rpc.ts';

// The same trading shell with a rewards panel: a stream of 3,600 reward tokens over an hour (one a second)
// is paid to registered holders in proportion to balance and time. Register and Claim are ordinary
// instructions of the hook program, simulated before the wallet signs, like a swap.
test.skip(DEVNET, 'the holder-rewards pool is only built for the local validator');

test.describe('Holder Rewards pool (CPMM)', () => {
  test.describe.configure({ mode: 'serial' });
  const fixture = load('holder-rewards', 'cpmm');
  const open = (page: Parameters<typeof openAndConnect>[0]) => openAndConnect(page, fixture);

  test('shows the funded stream and that this wallet has not registered', async ({ page }) => {
    await open(page);
    await expect(page.getByText('Holder rewards', { exact: true })).toBeVisible();
    await expect(page.getByTestId('policy-phase')).toContainText('Paying 1');
    await expect(page.getByTestId('registered')).toHaveText('No');
    await expect(page.getByRole('button', { name: 'Register' })).toBeEnabled();
    await expect(page.getByRole('button', { name: 'Claim' })).toBeDisabled();
  });

  test('registering starts the account earning, and the stream can then be claimed into the wallet', async ({ page }) => {
    await open(page);
    await page.getByRole('button', { name: 'Register' }).click();
    await expect(page.getByTestId('action-status')).toContainText('Registered');
    await expect(page.getByTestId('registered')).toHaveText('Yes');

    // The registered balance is the whole eligible supply but the pool's own, so the wallet earns the
    // whole stream: about one token a second. Wait until a little has accrued.
    await expect.poll(async () => Number(await page.getByTestId('claimable').textContent()), { timeout: 30_000 }).toBeGreaterThan(2);
    const before = await balanceOf(fixture, fixture.quote_mint);
    await page.getByRole('button', { name: 'Claim' }).click();
    await expect(page.getByTestId('action-status')).toContainText('Rewards claimed');
    const after = await balanceOf(fixture, fixture.quote_mint);
    expect(after - before).toBeGreaterThanOrEqual(2_000_000n);
    // what was claimed is gone from "claimable"
    await expect.poll(async () => Number(await page.getByTestId('claimable').textContent()), { timeout: 10_000 }).toBeLessThan(3);
  });

  test('registering twice is not offered, and a swap carries the three writable reward accounts', async ({ page }) => {
    await open(page);
    await expect(page.getByRole('button', { name: 'Register' })).toBeDisabled();
    const before = await balanceOf(fixture, fixture.hooked_mint);
    await page.getByRole('button', { name: 'Switch direction' }).click();
    await page.getByLabel('From').fill('10');
    await page.getByRole('button', { name: 'Swap', exact: true }).click();
    await expect(page.getByText('Swap confirmed')).toBeVisible();
    expect(await balanceOf(fixture, fixture.hooked_mint)).toBeGreaterThan(before);
    await page.getByText('Developer details').click();
    // the global, the two accounts' records, the hook program and its validation list
    await expect(page.getByTestId('developer-details')).toContainText('Output hook accounts (5)');
    // the registered holder keeps earning on the balance it now holds
    await expect(page.getByTestId('registered')).toHaveText('Yes');
  });
});
