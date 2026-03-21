<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { trackDependencies } from "$lib/shared/trackDependencies";

  let {
    text = "",
    active = true,
    amplitude = 22,
    speed = 0.3,
    phaseStep = 0.7,
    respectReducedMotion = true,
    startDelayMs = 0,
  }: {
    text?: string;
    active?: boolean;
    amplitude?: number;
    speed?: number;
    phaseStep?: number;
    respectReducedMotion?: boolean;
    startDelayMs?: number;
  } = $props();

  let letters = $derived(Array.from(text));
  let letterRefs: Array<HTMLSpanElement | null> = [];
  let mounted = false;
  let reduceMotion = false;
  let mediaQuery: MediaQueryList | null = null;
  let rafId: number | null = null;
  let startTimerId: number | null = null;
  let startMs = 0;
  let phaseOffsets: number[] = [];

  function captureLetter(node: HTMLSpanElement, index: number) {
    letterRefs[index] = node;
    return {
      destroy() {
        if (letterRefs[index] === node) {
          letterRefs[index] = null;
        }
      },
    };
  }

  function isMotionBlocked(): boolean {
    return respectReducedMotion && reduceMotion;
  }

  function applyRestState() {
    for (const letterRef of letterRefs) {
      if (!letterRef) continue;
      letterRef.style.transform = "translate3d(0, 0px, 0)";
      letterRef.style.willChange = "";
    }
  }

  function clearStartTimer() {
    if (startTimerId !== null) {
      clearTimeout(startTimerId);
      startTimerId = null;
    }
  }

  function stopAnimation() {
    clearStartTimer();
    if (rafId !== null) {
      cancelAnimationFrame(rafId);
      rafId = null;
    }
    startMs = 0;
    applyRestState();
  }

  function frame(now: number) {
    if (!mounted || !active || isMotionBlocked()) {
      stopAnimation();
      return;
    }

    if (startMs === 0) startMs = now - startDelayMs;
    const t = (now - startMs) / 1000;
    const omega = speed * Math.PI * 2;
    for (let i = 0; i < letterRefs.length; i += 1) {
      const letterRef = letterRefs[i];
      if (!letterRef) continue;
      const offset = Math.sin((t * omega) + (phaseOffsets[i] ?? 0)) * amplitude;
      letterRef.style.transform = `translate3d(0, ${offset}px, 0)`;
    }

    rafId = requestAnimationFrame(frame);
  }

  function startAnimation() {
    if (rafId !== null || startTimerId !== null || !mounted || !active || isMotionBlocked()) return;

    const beginAnimation = () => {
      if (!mounted || !active || isMotionBlocked()) {
        applyRestState();
        return;
      }
      for (const letterRef of letterRefs) {
        if (!letterRef) continue;
        letterRef.style.willChange = "transform";
      }
      startMs = 0;
      rafId = requestAnimationFrame(frame);
    };

    if (startDelayMs > 0) {
      startTimerId = window.setTimeout(() => {
        startTimerId = null;
        beginAnimation();
      }, startDelayMs);
      return;
    }

    beginAnimation();
  }

  function handleReduceMotionChange(e: MediaQueryListEvent) {
    reduceMotion = e.matches;
  }

  $effect(() => {
    trackDependencies(letters, phaseStep);
    letterRefs.length = letters.length;
    phaseOffsets = Array.from({ length: letters.length }, (_, i) => i * phaseStep);
  });

  $effect(() => {
    trackDependencies(active, reduceMotion, respectReducedMotion, startDelayMs);
    if (!mounted) return;
    if (active && !isMotionBlocked()) {
      startAnimation();
    } else {
      stopAnimation();
    }
  });

  $effect(() => {
    trackDependencies(text);
    if (!mounted) return;
    void tick().then(() => {
      if (!mounted || rafId !== null || startTimerId !== null) return;
      applyRestState();
    });
  });

  onMount(() => {
    mounted = true;
    if (typeof window.matchMedia === "function") {
      mediaQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
      reduceMotion = mediaQuery.matches;
      if (typeof mediaQuery.addEventListener === "function") {
        mediaQuery.addEventListener("change", handleReduceMotionChange);
      } else if (typeof mediaQuery.addListener === "function") {
        mediaQuery.addListener(handleReduceMotionChange);
      }
    }
    if (active && !isMotionBlocked()) startAnimation();
  });

  onDestroy(() => {
    mounted = false;
    stopAnimation();
    if (!mediaQuery) return;
    if (typeof mediaQuery.removeEventListener === "function") {
      mediaQuery.removeEventListener("change", handleReduceMotionChange);
    } else if (typeof mediaQuery.removeListener === "function") {
      mediaQuery.removeListener(handleReduceMotionChange);
    }
  });
</script>

<span class="wave-text" aria-label={text}>
  {#each letters as ch, i (i)}
    <span
      class="wave-char"
      use:captureLetter={i}
    >
      {ch === " " ? "\u00A0" : ch}
    </span>
  {/each}
</span>

<style>
  .wave-text {
    display: inline-flex;
    align-items: baseline;
    white-space: nowrap;
    font-family: "VT323", "Courier New", monospace !important;
    font-size: 1.18em;
    font-weight: 700;
    line-height: 0.9;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    color: color-mix(in srgb, var(--afk-text) 35%, transparent);
    text-shadow:
      0 0 1px color-mix(in srgb, var(--afk-text) 50%, transparent),
      0 0 4px color-mix(in srgb, var(--afk-text) 38%, transparent),
      0 0 16px color-mix(in srgb, var(--afk-text) 75%, transparent),
      0 0 34px color-mix(in srgb, var(--afk-text) 58%, transparent),
      0 0 52px color-mix(in srgb, var(--afk-text) 42%, transparent);
  }

  .wave-char {
    display: inline-block;
    transform: translate3d(0, 0px, 0);
  }
</style>
