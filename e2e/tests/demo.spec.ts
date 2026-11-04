import { expect, test } from '@playwright/test';

test('the demo explains itself and shows seeded servers', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByText('Demo data · no backend')).toBeVisible();
  // Seeded servers are listed in the sidebar.
  await expect(page.getByRole('button', { name: /survival-1/ }).first()).toBeVisible();
});

test('settings report demo mode honestly', async ({ page }) => {
  await page.goto('/?page=app-settings');
  await expect(page.getByText('Demo data').first()).toBeVisible();
  await expect(page.getByText('Nothing you do here runs a real server')).toBeVisible();
});

test('a CurseForge file that must be downloaded by hand', async ({ page }) => {
  // CurseForge waits for an API key.
  await page.goto('/?page=app-settings');
  await page.getByLabel('CurseForge API key').fill('demo-key');
  await page.getByRole('button', { name: 'Save key' }).click();
  await expect(page.getByText('Key set')).toBeVisible();

  // The demo lives in memory, so stay in the app rather than reloading.
  await page.getByRole('button', { name: 'Configurations' }).click();
  await page.getByRole('button', { name: 'Paper 1.21.4 · Baseline' }).first().click();
  await page.getByRole('button', { name: 'Mods & Plugins' }).or(page.getByRole('link', { name: 'Mods & Plugins' })).first().click();
  await page.getByRole('tab', { name: 'Browse' }).click();
  await page.getByRole('radio', { name: /CurseForge/ }).click();
  await page.getByLabel('Search CurseForge plugins').fill('essentials');
  await expect(page.getByRole('link', { name: 'WorldEdit', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: 'Install', exact: true }).first().click();

  const plan = page.getByRole('dialog', { name: 'Install EssentialsX' });
  await expect(plan.getByText('Manual download', { exact: true }).first()).toBeVisible();
  await plan.getByRole('button', { name: 'Install', exact: true }).click();
  await expect(plan).toBeHidden();

  // Download it, then hand it back.
  await expect(page.getByText('Download this file yourself.')).toBeVisible();
  const download = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Download' }).or(page.getByRole('link', { name: 'Download' })).first().click();
  const file = await (await download).path();
  const chooser = page.waitForEvent('filechooser');
  await page.getByRole('button', { name: 'Upload file' }).click();
  await (await chooser).setFiles(file);
  await expect(page.getByText('Download this file yourself.')).toHaveCount(0);
});
