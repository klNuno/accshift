export function clampInt(value: number, min: number, max: number, fallback: number): number {
  if (!Number.isFinite(value)) return fallback;
  return Math.min(max, Math.max(min, Math.round(value)));
}

export function createNumericInput(
  getter: () => number,
  setter: (v: number) => void,
  min: number,
  max: number,
) {
  let input = $state(String(getter()));

  return {
    get input() {
      return input;
    },
    set input(v: string) {
      input = v;
    },
    commit() {
      const clamped = clampInt(Number(input), min, max, getter());
      setter(clamped);
      input = String(clamped);
    },
    refresh() {
      input = String(getter());
    },
  };
}
