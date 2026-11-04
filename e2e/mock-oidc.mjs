#!/usr/bin/env node
/**
 * Just enough of an OpenID Connect provider (standing in for Authentik) for
 * the `oidc` Playwright project: discovery, JWKS, an authorize endpoint that
 * approves at once, a token endpoint that checks PKCE and returns an RS256
 * ID token with the sign-in's nonce, and an end-session page.
 *
 *   node mock-oidc.mjs 7991
 *
 * `POST /test/user {sub, preferred_username, name, groups}` picks who the
 * next sign-in is.
 */

import { createHash, createSign, generateKeyPairSync, randomBytes } from 'node:crypto';
import { createServer } from 'node:http';

const port = Number(process.argv[2] ?? 7991);
const base = `http://127.0.0.1:${port}`;
const issuer = `${base}/application/o/lodestone/`;
const { privateKey, publicKey } = generateKeyPairSync('rsa', { modulusLength: 2048 });
const jwk = { ...publicKey.export({ format: 'jwk' }), kid: 'e2e', alg: 'RS256', use: 'sig' };

const ADMIN = { sub: 'e2e-admin', preferred_username: 'riley', name: 'Riley Admin', groups: ['minecraft', 'homelab-admins'] };
let nextUser = ADMIN;
const codes = new Map();

const b64url = (value) => Buffer.from(value).toString('base64url');

function idToken(user, nonce) {
  const now = Math.floor(Date.now() / 1000);
  const header = b64url(JSON.stringify({ alg: 'RS256', typ: 'JWT', kid: 'e2e' }));
  const payload = b64url(JSON.stringify({ iss: issuer, aud: 'lodestone', iat: now, exp: now + 600, nonce, ...user }));
  const signature = createSign('RSA-SHA256').update(`${header}.${payload}`).sign(privateKey);
  return `${header}.${payload}.${b64url(signature)}`;
}

const json = (res, status, body) => {
  res.writeHead(status, { 'content-type': 'application/json' });
  res.end(JSON.stringify(body));
};

const readBody = (req) =>
  new Promise((resolve) => {
    let body = '';
    req.on('data', (chunk) => (body += chunk));
    req.on('end', () => resolve(body));
  });

createServer(async (req, res) => {
  const url = new URL(req.url, base);
  if (url.pathname === '/application/o/lodestone/.well-known/openid-configuration') {
    return json(res, 200, {
      issuer,
      jwks_uri: `${base}/jwks`,
      authorization_endpoint: `${base}/application/o/authorize/`,
      token_endpoint: `${base}/application/o/token/`,
      end_session_endpoint: `${base}/application/o/lodestone/end-session/`,
    });
  }
  if (url.pathname === '/jwks') return json(res, 200, { keys: [jwk] });
  if (url.pathname === '/application/o/authorize/') {
    const q = url.searchParams;
    if (q.get('client_id') !== 'lodestone' || q.get('code_challenge_method') !== 'S256') {
      return json(res, 400, { error: 'invalid_request' });
    }
    const code = randomBytes(16).toString('hex');
    codes.set(code, { challenge: q.get('code_challenge'), redirectUri: q.get('redirect_uri'), nonce: q.get('nonce'), user: nextUser });
    const back = new URL(q.get('redirect_uri'));
    back.searchParams.set('code', code);
    back.searchParams.set('state', q.get('state'));
    res.writeHead(302, { location: back.toString() });
    return res.end();
  }
  if (url.pathname === '/application/o/token/' && req.method === 'POST') {
    const form = new URLSearchParams(await readBody(req));
    const grant = codes.get(form.get('code'));
    codes.delete(form.get('code'));
    const challenge = createHash('sha256').update(form.get('code_verifier') ?? '').digest('base64url');
    if (!grant || grant.challenge !== challenge || grant.redirectUri !== form.get('redirect_uri')) {
      return json(res, 400, { error: 'invalid_grant' });
    }
    return json(res, 200, { access_token: 'unused', token_type: 'Bearer', expires_in: 600, id_token: idToken(grant.user, grant.nonce) });
  }
  if (url.pathname === '/application/o/lodestone/end-session/') {
    res.writeHead(200, { 'content-type': 'text/html' });
    return res.end('<h1>Signed out of the identity provider</h1>');
  }
  if (url.pathname === '/test/user' && req.method === 'POST') {
    nextUser = { ...ADMIN, ...JSON.parse(await readBody(req)) };
    return json(res, 200, nextUser);
  }
  if (url.pathname === '/health') return json(res, 200, { ok: true });
  json(res, 404, { error: 'not_found' });
}).listen(port, '127.0.0.1', () => console.log(`mock OIDC provider at ${issuer}`));
