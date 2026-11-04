import { expect, test } from '@playwright/test';

/**
 * Sign-in the way it works against Authentik: the app sends the browser to
 * the backend's /auth/login, the provider approves, the backend checks the
 * ID token and starts a session. Each person sees only their own assets.
 */

const PROVIDER = 'http://127.0.0.1:7991';

test('signing in, owning a world, and signing out', async ({ page, request }) => {
  await request.post(`${PROVIDER}/test/user`, {
    data: { sub: 'e2e-admin', preferred_username: 'riley', name: 'Riley Admin', groups: ['homelab-admins'] },
  });
  await page.goto('/?page=worlds');
  // Bounced through the provider and back, signed in.
  await expect(page).toHaveURL(/\?page=worlds/);
  await expect(page.getByText('Riley Admin')).toBeVisible();
  await expect(page.getByText('@riley · admin')).toBeVisible();

  await page.getByRole('button', { name: 'Create world' }).click();
  await page.getByRole('dialog').getByLabel('Name').fill('Riley’s World');
  await page.getByRole('dialog').getByRole('button', { name: 'Create world' }).click();
  await expect(page.getByText('Riley’s World').first()).toBeVisible();

  // Sign out: the session ends and the provider's sign-out page follows.
  await page.getByRole('button', { name: /Riley Admin/ }).click();
  await page.getByRole('menuitem', { name: 'Sign out' }).click();
  await expect(page.getByText('Signed out of the identity provider')).toBeVisible();

  // Someone else signs in and doesn't see Riley's world.
  await request.post(`${PROVIDER}/test/user`, {
    data: { sub: 'e2e-alex', preferred_username: 'alex', name: 'Alex Player', groups: ['minecraft'] },
  });
  await page.goto('/?page=worlds');
  await expect(page.getByText('Alex Player')).toBeVisible();
  await expect(page.getByText('@alex', { exact: true })).toBeVisible();
  await expect(page.getByText('No worlds yet')).toBeVisible();
  await expect(page.getByText('Riley’s World')).toHaveCount(0);
});
