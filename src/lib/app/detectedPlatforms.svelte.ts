/**
 * What launcher detection found this launch.
 *
 * Set once, during boot, and only on a fresh install: detection runs to pick
 * the platforms enabled by default. The onboarding reads it so its first
 * screen lists what is actually on the machine rather than what the OS could
 * support. Empty means detection did not run, or ran and found nothing, and
 * callers fall back to the compatibility list.
 */
let detected = $state<string[]>([]);

export function setDetectedPlatforms(platformIds: string[]) {
  detected = [...platformIds];
}

export function getDetectedPlatforms(): string[] {
  return detected;
}
