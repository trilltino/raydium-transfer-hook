import { expect, test } from '@playwright/test';
import { connectTestWallet, load, secret } from './helpers.ts';
import { ENV_NAME } from './rpc.ts';

// Under the search bar of the pool picker: make a demo pool, or check a token of your own. Against the local
// validator the dev server makes the pool with the committed fixture admin; the devnet version needs the deployer
// key and spends devnet SOL, so it is not part of the browser suite.
test.skip(ENV_NAME !== 'localnet', 'pool creation here uses the local validator');

async function openPicker(page: import('@playwright/test').Page): Promise<void> {
  await page.goto(`/?env=${ENV_NAME}&testWallet=${encodeURIComponent(JSON.stringify(secret))}`);
  await connectTestWallet(page);
  await expect(page.getByText('Open a pool')).toBeVisible();
}

test.describe('under the search bar', () => {
  test('Create a demo pool makes a hooked token and a pool, and opens it', async ({ page }) => {
    test.setTimeout(300_000);
    await openPicker(page);
    await page.getByRole('button', { name: 'Create a demo pool' }).click();
    await expect(page.getByRole('button', { name: 'Create pool' })).toBeEnabled();
    await page.getByRole('combobox').nth(1).selectOption('cpmm');
    await page.getByRole('button', { name: 'Create pool' }).click();
    await expect(page.getByTestId('create-pool-progress')).toBeVisible();
    // The run ends on the new pool's own page: its policy, and the pool in the address.
    await expect(page.getByTestId('policy-rules')).toBeVisible({ timeout: 240_000 });
    expect(page.url()).toMatch(/[?&]pool=[1-9A-HJ-NP-Za-km-z]{32,44}/);
    // The connected wallet was given tokens when the pool was made, so it can trade it at once.
    await page.getByRole('button', { name: 'I understand, continue' }).first().click().catch(() => undefined);
    await expect(page.getByTestId('from-balance')).not.toHaveText('Balance: 0');
  });

  test('Bring your own token reads a hooked mint, its approval on each AMM, and the next commands', async ({ page }) => {
    const fixture = load('fair-launch', 'cpmm');
    await page.goto(`/?env=${ENV_NAME}`);
    await page.getByRole('button', { name: 'Bring your own token' }).click();
    await expect(page.getByTestId('command').first()).toContainText('--hook-dir ./your-hook');

    await page.getByPlaceholder('Token mint address').fill(fixture.hooked_mint);
    await page.getByRole('button', { name: 'Check token' }).click();
    const check = page.getByTestId('token-check');
    await expect(check).toHaveAttribute('data-kind', 'hooked');
    await expect(check).toContainText(fixture.hook_program);
    // The pool was made with the hooked mint approved on its AMM; the other AMM is up to the admin.
    await expect(page.getByTestId('token-approval').locator('li').first()).toHaveAttribute('data-status', 'approved');
    await expect(page.getByTestId('command').filter({ hasText: 'mint approval' })).toHaveCount(1);

    await page.getByPlaceholder('Token mint address').fill(fixture.quote_mint);
    await page.getByRole('button', { name: 'Check token' }).click();
    await expect(check).toHaveAttribute('data-kind', 'plain');
    await expect(check).toContainText('no Transfer Hook');

    await page.getByPlaceholder('Token mint address').fill('not an address');
    await page.getByRole('button', { name: 'Check token' }).click();
    await expect(check).toHaveAttribute('data-kind', 'invalid');
  });
});
