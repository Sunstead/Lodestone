/**
 * Read-only metric snapshots for the demo backend.
 *
 * `demo.ts` owns the state and the drift; this module just exposes a
 * snapshot function so polling the status endpoint stays cheap and the
 * client does not have to re-export the whole demo module.
 */

import * as demo from './demo';
import type { InstanceStatus } from './types';

export function getStatusSnapshot(id: string): InstanceStatus {
  return demo.snapshotStatus(id);
}
