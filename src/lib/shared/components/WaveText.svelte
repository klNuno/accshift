<script lang="ts">
  import { onDestroy, onMount } from "svelte";

  let {
    text = "",
    active = true,
    amplitude = 22,
    speed = 0.3,
    phaseStep = 0.7,
    respectReducedMotion = true,
  }: {
    text?: string;
    active?: boolean;
    amplitude?: number;
    speed?: number;
    phaseStep?: number;
    respectReducedMotion?: boolean;
  } = $props();

  let letters = $derived(Array.from(text));
  let offsets = $state<number[]>([]);

  let mounted = false;
  let reduceMotion = false;
  let mediaQuery: MediaQueryList | null = null;
  let rafId: number | null = null;
  let startMs = 0;

  function applyRestState() {
    offsets = Array.from({ length: letters.length }, () => 0);
  }

  function stopAnimation() {
    if (rafId !== null) {
      cancelAnimationFrame(rafId);
      rafId = null;
    }
    applyRestState();
  }

  function frame(now: number) {
    const blockedByMotionPref = respectReducedMotion && reduceMotion;
    if (!mounted || !active || blockedByMotionPref) {
      stopAnimation();
      return;
    }

    if (startMs === 0) startMs = now;
    const t = (now - startMs) / 1000;
    const omega = speed * Math.PI * 2;
    offsets = Array.from({ length: letters.length }, (_, i) =>
      Math.sin((t * omega) + (i * phaseStep)) * amplitude
    );

    rafId = requestAnimationFrame(frame);
  }

  function startAnimation() {
    const blockedByMotionPref = respectReducedMotion && reduceMotion;
    if (rafId !== null || !mounted || !active || blockedByMotionPref) return;
    startMs = 0;
    rafId = requestAnimationFrame(frame);
  }

  function handleReduceMotionChange(e: MediaQueryListEvent) {
    reduceMotion = e.matches;
  }

  $effect(() => {
    text;
    offsets = Array.from({ length: Array.from(text).length }, () => 0);
  });

  $effect(() => {
    active;
    reduceMotion;
    respectReducedMotion;
    if (!mounted) return;
    const blockedByMotionPref = respectReducedMotion && reduceMotion;
    if (active && !blockedByMotionPref) {
      startAnimation();
    } else {
      stopAnimation();
    }
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
    const blockedByMotionPref = respectReducedMotion && reduceMotion;
    if (active && !blockedByMotionPref) startAnimation();
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
      style={`transform: translateY(${(offsets[i] ?? 0).toFixed(2)}px);`}
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
    will-change: transform;
    transform: translateY(0px);
  }
</style>
