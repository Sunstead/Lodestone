import { describe, expect, it } from 'vitest';
import { keys, keysForEvent } from './queries';

describe('keysForEvent', () => {
  it('refreshes instances when a world changes, since instances render world names', () => {
    const stale = keysForEvent({ type: 'changed', resource: 'worlds', id: 'world-1' });
    expect(stale).toContainEqual(keys.worlds);
    expect(stale).toContainEqual(keys.instances);
  });

  it('targets the instance on a state change', () => {
    const stale = keysForEvent({ type: 'instanceState', id: 'i-1', state: 'running' });
    expect(stale).toContainEqual(keys.instance('i-1'));
    expect(stale).toContainEqual(keys.instanceStatus('i-1'));
  });

  it('scopes backup changes to the world', () => {
    expect(keysForEvent({ type: 'changed', resource: 'backups', id: 'world-9' })).toContainEqual(keys.worldBackups('world-9'));
  });
});
