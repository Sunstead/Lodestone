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
