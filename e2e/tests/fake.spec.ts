import { expect, test } from '@playwright/test';

/**
 * Full stack: browser → built frontend → Lodestone backend → simulated
 * runtime (real RCON, real SQLite, real files on disk).
 */

test('launch a world, use the console, stop the server', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByText('Demo data')).toHaveCount(0);

  await page.getByRole('button', { name: 'Launch world' }).first().click();
  await expect(page.getByRole('dialog', { name: 'Launch a world' })).toBeVisible();
  await page.getByLabel('World name').fill('E2E Survival');
  await page.getByRole('button', { name: 'Launch server' }).click();
  await page.getByRole('button', { name: 'Open server' }).click();

  // Preparing → starting → online, driven by real backend events.
  await expect(page.getByText('Online').first()).toBeVisible({ timeout: 30_000 });

  await page.getByRole('link', { name: 'Console' }).or(page.getByRole('button', { name: 'Console' })).first().click();
  const input = page.getByLabel('Console command');
  await input.fill('list');
  await input.press('Enter');
  await expect(page.getByLabel('Server console output').getByText(/players online/)).toBeVisible();

  await page.getByRole('button', { name: 'Stop' }).click();
  await expect(page.getByText('Offline').first()).toBeVisible({ timeout: 30_000 });
});

test('settings show the live backend and simulated runtime', async ({ page }) => {
  await page.goto('/?page=app-settings');
  await expect(page.getByText('Live API')).toBeVisible();
  await expect(page.getByText('Simulated servers: nothing is downloaded or run.')).toBeVisible();
  await expect(page.getByRole('switch', { name: 'I have read and accept the Minecraft EULA' })).toBeChecked();
});
