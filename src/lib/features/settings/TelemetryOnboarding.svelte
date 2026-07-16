<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { MessageKey, TranslationParams } from "$lib/i18n";
  import tradeOfferVideo from "../../../assets/trade-offer.webm";
  import noThanksVideo from "../../../assets/no-thanks.webm";
  import logoUrl from "/logo.svg";

  const REJECT_FADE_DELAY_MS = 1200;
  const REJECT_TOTAL_MS = 2000;

  type Step = "welcome" | "features" | "deal";
  type PlatformLike = { id: string; name: string };

  let {
    t,
    version,
    compatiblePlatforms,
    onTourActive,
    onComplete,
  }: {
    t: (key: MessageKey, params?: TranslationParams) => string;
    version: string;
    compatiblePlatforms: PlatformLike[];
    onTourActive: (active: boolean) => void;
    onComplete: () => void;
  } = $props();

  let step = $state<Step>("welcome");
  let submitting = $state(false);
  let rejecting = $state(false);
  let fadingOut = $state(false);
  let titlebarH = $state(0);
  let dealTitleEl = $state<HTMLHeadingElement | null>(null);
  let gifEl = $state<HTMLVideoElement | null>(null);
  let gifSize = $state<{ w: number; h: number } | null>(null);

  $effect(() => {
    onTourActive(step === "features");
  });

  // WAAPI rather than a CSS transition: the transition raced the mount and
  // jumped straight to red. cancel() on cleanup resets to white for revisits.
  $effect(() => {
    if (step !== "deal" || !dealTitleEl) return;
    if (document.documentElement.dataset.motion === "reduced") {
      dealTitleEl.style.color = "#ef4444";
      return () => dealTitleEl?.style.removeProperty("color");
    }
    // Midpoint keyframe: plain white->red sRGB lerp reads as "nothing happens
    // then sudden red"; forcing a visible pink at 40% spreads the shift out.
    const anim = dealTitleEl.animate(
      [
        { color: "#ffffff", textShadow: "0 0 0px rgba(239, 68, 68, 0)" },
        { color: "#f0a3a3", textShadow: "0 0 6px rgba(239, 68, 68, 0.2)", offset: 0.4 },
        { color: "#ef4444", textShadow: "0 0 18px rgba(239, 68, 68, 0.6)" },
      ],
      { duration: 10000, easing: "linear", fill: "forwards" },
    );
    return () => anim.cancel();
  });

  async function finish(modeA: boolean, modeB: boolean) {
    if (submitting) return;
    submitting = true;
    try {
      await invoke("telemetry_complete_onboarding", {
        modeAEnabled: modeA,
        modeBEnabled: modeB,
      });
    } catch (e) {
      console.error("telemetry_complete_onboarding failed", e);
    } finally {
      submitting = false;
      onComplete();
    }
  }

  function handleNo() {
    if (submitting || rejecting) return;
    if (gifEl) {
      const r = gifEl.getBoundingClientRect();
      gifSize = { w: r.width, h: r.height };
    }
    rejecting = true;
    setTimeout(() => { fadingOut = true; }, REJECT_FADE_DELAY_MS);
    setTimeout(() => { void finish(false, false); }, REJECT_TOTAL_MS);
  }
  function handleBasic() { void finish(true, false); }
  function handleDeal() { void finish(true, true); }

  function goWelcome() { step = "welcome"; }
  function goFeatures() { step = "features"; }
  function goDeal() { step = "deal"; }

  type Feature = {
    id: string;
    selector: string;
    labelKey: MessageKey;
    bodyKey: MessageKey;
    showContextMenu?: boolean;
  };

  const FEATURES: Feature[] = [
    {
      id: "addAccount",
      selector: '[data-tour="add-account"]',
      labelKey: "onboarding.features.addAccount.label",
      bodyKey: "onboarding.features.addAccount.body",
    },
    {
      id: "switch",
      selector: '[data-tour="account-card"]',
      labelKey: "onboarding.features.switch.label",
      bodyKey: "onboarding.features.switch.body",
    },
    {
      id: "contextMenu",
      selector: '[data-tour="account-card"]',
      labelKey: "onboarding.features.contextMenu.label",
      bodyKey: "onboarding.features.contextMenu.body",
      showContextMenu: true,
    },
    {
      id: "platformSwitch",
      selector: '[data-tour="platforms"]',
      labelKey: "onboarding.features.platformSwitch.label",
      bodyKey: "onboarding.features.platformSwitch.body",
    },
  ];

  let activeFeatureIdx = $state(0);
  let spotlightRect = $state<{ x: number; y: number; w: number; h: number } | null>(null);
  let contextMenuPos = $state<{ x: number; y: number } | null>(null);
  let spotlightMissing = $state(false);

  function unionRect(a: DOMRect, b: DOMRect): DOMRect {
    const left = Math.min(a.left, b.left);
    const top = Math.min(a.top, b.top);
    const right = Math.max(a.right, b.right);
    const bottom = Math.max(a.bottom, b.bottom);
    return new DOMRect(left, top, right - left, bottom - top);
  }

  function measureCurrent() {
    if (step !== "features") {
      spotlightRect = null;
      contextMenuPos = null;
      return;
    }
    const f = FEATURES[activeFeatureIdx];
    const el = document.querySelector<HTMLElement>(f.selector);
    if (!el) {
      spotlightRect = null;
      contextMenuPos = null;
      spotlightMissing = true;
      return;
    }
    spotlightMissing = false;
    let rect: DOMRect = el.getBoundingClientRect();
    if (f.showContextMenu) {
      contextMenuPos = { x: rect.right + 10, y: rect.top + 8 };
      const ctxEl = document.querySelector<HTMLElement>(".mock-ctx-menu");
      if (ctxEl) {
        rect = unionRect(rect, ctxEl.getBoundingClientRect());
      }
    } else {
      contextMenuPos = null;
    }
    spotlightRect = {
      x: rect.left - 6,
      y: rect.top - 6,
      w: rect.width + 12,
      h: rect.height + 12,
    };
  }

  function measureTitlebar() {
    const el = document.querySelector<HTMLElement>(".titlebar");
    titlebarH = el ? el.getBoundingClientRect().height : 0;
  }

  $effect(() => {
    void activeFeatureIdx;
    void step;
    tick().then(() => {
      measureCurrent();
      requestAnimationFrame(measureCurrent);
    });
  });

  onMount(() => {
    measureTitlebar();
    const onResize = () => {
      measureCurrent();
      measureTitlebar();
    };
    window.addEventListener("resize", onResize);
    window.addEventListener("scroll", onResize, true);
    const tbEl = document.querySelector<HTMLElement>(".titlebar");
    let ro: ResizeObserver | null = null;
    if (tbEl && typeof ResizeObserver !== "undefined") {
      ro = new ResizeObserver(() => measureTitlebar());
      ro.observe(tbEl);
    }
    return () => {
      window.removeEventListener("resize", onResize);
      window.removeEventListener("scroll", onResize, true);
      ro?.disconnect();
    };
  });

  function nextFeature() {
    if (activeFeatureIdx < FEATURES.length - 1) {
      activeFeatureIdx += 1;
    } else {
      goDeal();
    }
  }
  function prevFeature() {
    if (activeFeatureIdx > 0) {
      activeFeatureIdx -= 1;
    } else {
      goWelcome();
    }
  }
</script>

<div
  class="click-shield"
  class:fading={fadingOut}
  style={`top:${titlebarH}px;`}
  aria-hidden="true"
></div>

{#if step === "features" && contextMenuPos}
  <div
    class="mock-ctx-menu"
    style={`left:${contextMenuPos.x}px;top:${contextMenuPos.y}px;`}
    role="presentation"
  >
    <div class="mock-ctx-item">{t("onboarding.features.mockRename")}</div>
    <div class="mock-ctx-item">{t("onboarding.features.mockSetColor")}</div>
    <div class="mock-ctx-item">{t("onboarding.features.mockMoveToFolder")}</div>
    <div class="mock-ctx-sep"></div>
    <div class="mock-ctx-item danger">{t("onboarding.features.mockDelete")}</div>
  </div>
{/if}

{#if step === "features" && spotlightRect}
  <div
    class="spotlight"
    style={`left:${spotlightRect.x}px;top:${spotlightRect.y}px;width:${spotlightRect.w}px;height:${spotlightRect.h}px;`}
  ></div>
{/if}

<div
  class="backdrop"
  class:dim={step !== "features"}
  class:clear={step === "features"}
  class:fading={fadingOut}
>
  <div
    class="modal"
    class:dock-bottom={step === "features"}
    class:deal-mode={step === "deal"}
    class:fading={fadingOut}
    role="dialog"
    aria-modal="true"
    aria-labelledby="onboarding-title"
  >
    <div class="dots" aria-hidden="true">
      <span class:on={step === "welcome"}></span>
      <span class:on={step === "features"}></span>
      <span class:on={step === "deal"}></span>
    </div>

    {#if step === "welcome"}
      <div class="step">
        <div class="hero">
          <img class="logo" src={logoUrl} alt="Accshift" />
          <h2 id="onboarding-title">
            {t("onboarding.welcome.title")}
            <span class="version-plain">{t("onboarding.welcome.version", { version: version || "?" })}</span>
          </h2>
          <p class="compat-label">{t("onboarding.welcome.compatibleWith")}</p>
          <ul class="compat-list">
            {#each compatiblePlatforms as p (p.id)}
              <li class="compat-chip">{p.name}</li>
            {/each}
          </ul>
        </div>
        <div class="actions split">
          <button type="button" class="ghost" onclick={handleBasic} disabled={submitting}>
            {t("onboarding.welcome.skip")}
          </button>
          <button type="button" class="primary" onclick={goFeatures}>
            {t("onboarding.welcome.next")}
          </button>
        </div>
      </div>
    {:else if step === "features"}
      <div class="step features-step">
        <div class="features-head">
          <h2 id="onboarding-title">{t("onboarding.features.title")}</h2>
          <div class="step-counter">{activeFeatureIdx + 1} / {FEATURES.length}</div>
        </div>

        <div class="feature-bare">
          <div class="feature-accent"></div>
          <div class="feature-text-wrap">
            <div class="feature-label">{t(FEATURES[activeFeatureIdx].labelKey)}</div>
            <p class="feature-body">{t(FEATURES[activeFeatureIdx].bodyKey)}</p>
          </div>
        </div>

        {#if spotlightMissing}
          <p class="feature-missing">{t("onboarding.features.hint")}</p>
        {/if}

        <div class="actions split">
          <button type="button" class="ghost" onclick={prevFeature}>
            {activeFeatureIdx === 0 ? t("onboarding.features.back") : "←"}
          </button>
          <div class="legend-dots" aria-hidden="true">
            {#each FEATURES as _, idx}
              <button
                type="button"
                class="legend-dot"
                class:on={idx === activeFeatureIdx}
                onclick={() => (activeFeatureIdx = idx)}
                aria-label={t("onboarding.features.step", { step: idx + 1 })}
              ></button>
            {/each}
          </div>
          <button type="button" class="primary" onclick={nextFeature}>
            {activeFeatureIdx === FEATURES.length - 1 ? t("onboarding.features.next") : "→"}
          </button>
        </div>
      </div>
    {:else}
      <div class="step">
        <h2 id="onboarding-title" class="deal-title" bind:this={dealTitleEl}>
          {t("onboarding.telemetry.title")}
        </h2>
        {#key rejecting}
          <video
            class="deal-gif"
            bind:this={gifEl}
            src={rejecting ? noThanksVideo : tradeOfferVideo}
            aria-label={t("onboarding.telemetry.gifAlt")}
            autoplay
            loop
            muted
            playsinline
            disablepictureinpicture
            style={gifSize ? `width:${gifSize.w}px;height:${gifSize.h}px;` : undefined}
          ></video>
        {/key}
        <p class="intro">{t("onboarding.telemetry.intro")}</p>
        <p class="question">{t("onboarding.telemetry.question")}</p>

        <div class="deal-buttons">
          <button
            type="button"
            class="deal-row no-btn"
            class:no-clicked={rejecting}
            disabled={submitting || rejecting}
            onclick={handleNo}
          >
            <div class="deal-row-label">{t("onboarding.telemetry.no")}</div>
            <div class="deal-row-body">{t("onboarding.telemetry.noHint")}</div>
          </button>
          <button
            type="button"
            class="deal-row"
            disabled={submitting || rejecting}
            onclick={handleBasic}
          >
            <div class="deal-row-label">
              {t("onboarding.telemetry.basic")}
              <span class="default-inline">{t("onboarding.telemetry.basicDefault")}</span>
            </div>
            <div class="deal-row-body">{t("onboarding.telemetry.basicHint")}</div>
          </button>
          <button
            type="button"
            class="deal-row deal-accent"
            disabled={submitting || rejecting}
            onclick={handleDeal}
          >
            <div class="deal-row-label">{t("onboarding.telemetry.deal")}</div>
            <div class="deal-row-body">{t("onboarding.telemetry.dealHint")}</div>
          </button>
        </div>

        <div class="actions split">
          <button type="button" class="ghost" onclick={goFeatures} disabled={submitting || rejecting}>
            {t("onboarding.telemetry.back")}
          </button>
          <span></span>
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .click-shield {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 8990;
    background: transparent;
    pointer-events: auto;
    transition: opacity 800ms ease-out;
  }
  .click-shield.fading { opacity: 0; }

  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 9000;
    animation: fadeIn 200ms ease-out;
    transition: background 280ms ease-out, backdrop-filter 280ms ease-out, opacity 800ms ease-out;
    pointer-events: none;
  }
  .backdrop.dim {
    background: color-mix(in srgb, #000 60%, transparent);
    backdrop-filter: blur(8px);
    pointer-events: auto;
    display: grid;
    place-items: center;
  }
  /* During the tour the spotlight box-shadow does the dimming; the backdrop
     goes transparent and above it (z 9003 > 9001) so the docked explanation
     card is not darkened. */
  .backdrop.clear {
    background: transparent;
    backdrop-filter: blur(0px);
    z-index: 9003;
  }
  .backdrop.fading { opacity: 0; }

  .spotlight {
    position: fixed;
    border-radius: 14px;
    pointer-events: none;
    z-index: 9001;
    box-shadow:
      0 0 0 9999px color-mix(in srgb, #000 55%, transparent),
      0 0 0 2px #60a5fa,
      0 0 28px color-mix(in srgb, #60a5fa 70%, transparent),
      inset 0 0 0 2px color-mix(in srgb, #60a5fa 90%, transparent);
    transition: left 260ms cubic-bezier(0.22, 1, 0.36, 1),
                top 260ms cubic-bezier(0.22, 1, 0.36, 1),
                width 260ms cubic-bezier(0.22, 1, 0.36, 1),
                height 260ms cubic-bezier(0.22, 1, 0.36, 1);
    animation: pulse 1.8s ease-in-out infinite;
  }

  .mock-ctx-menu {
    position: fixed;
    min-width: 170px;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: 0 18px 40px rgba(0, 0, 0, 0.55);
    padding: 4px;
    display: flex;
    flex-direction: column;
    z-index: 9001;
    animation: ctxIn 180ms ease-out;
    pointer-events: none;
  }
  .mock-ctx-item {
    padding: 7px 12px;
    border-radius: 6px;
    font-size: 12px;
    color: var(--fg);
  }
  .mock-ctx-item.danger { color: #ef4444; }
  .mock-ctx-sep {
    height: 1px;
    background: var(--border);
    margin: 4px 6px;
  }

  .modal {
    width: min(94vw, 540px);
    max-height: 92vh;
    overflow-y: auto;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 16px;
    padding: 22px 22px 20px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    color: var(--fg);
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.55);
    pointer-events: auto;
    animation: modalIn 280ms cubic-bezier(0.22, 1, 0.36, 1);
  }
  /* Transition only while fading out. Declared permanently it would also
     animate the dock-bottom translateX(-50%) on step changes, sliding the
     whole modal sideways over 800ms. */
  .modal.fading {
    opacity: 0;
    transform: scale(0.98) translateY(6px);
    transition: opacity 800ms ease-out, transform 800ms ease-out;
  }
  /* Deal step never scrolls: overflow is hidden and the gif is the flexible
     element that shrinks when the window is short. */
  .modal.deal-mode {
    overflow: hidden;
    gap: 12px;
    padding: 18px 22px 16px;
  }
  .modal.deal-mode .step {
    min-height: 0;
    gap: 10px;
  }
  .modal.dock-bottom {
    position: fixed;
    bottom: 24px;
    left: 50%;
    transform: translateX(-50%);
    width: min(94vw, 460px);
    padding: 16px 18px 14px;
    gap: 10px;
    z-index: 9002;
    animation: dockIn 280ms cubic-bezier(0.22, 1, 0.36, 1);
  }

  .dots {
    display: flex;
    gap: 6px;
    justify-content: center;
    margin-bottom: 2px;
  }
  .dots span {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: color-mix(in srgb, var(--fg) 25%, transparent);
    transition: background 160ms ease-out, transform 160ms ease-out;
  }
  .dots span.on {
    background: var(--fg);
    transform: scale(1.25);
  }

  .step {
    display: flex;
    flex-direction: column;
    gap: 14px;
    animation: stepIn 220ms ease-out;
  }
  .features-step { gap: 12px; }

  .features-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
  }
  .step-counter {
    font-size: 11px;
    font-weight: 600;
    color: var(--fg-subtle);
  }

  .feature-bare {
    display: flex;
    gap: 12px;
    padding: 4px 0 0 0;
    animation: featureSwap 220ms ease-out;
  }
  .feature-accent {
    flex: 0 0 auto;
    width: 3px;
    align-self: stretch;
    background: #60a5fa;
    border-radius: 2px;
  }
  .feature-text-wrap {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .feature-label {
    font-size: 14px;
    font-weight: 700;
  }
  .feature-body {
    margin: 0;
    font-size: 12px;
    line-height: 1.55;
    color: var(--fg-muted);
  }
  .feature-missing {
    margin: 0;
    font-size: 11px;
    color: var(--fg-subtle);
    font-style: italic;
    text-align: center;
  }

  .legend-dots {
    display: flex;
    gap: 6px;
  }
  .legend-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    border: none;
    padding: 0;
    background: color-mix(in srgb, var(--fg) 25%, transparent);
    cursor: pointer;
    transition: background 140ms ease-out, transform 140ms ease-out;
  }
  .legend-dot.on {
    background: #60a5fa;
    transform: scale(1.25);
  }

  .hero {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 10px;
    padding: 6px 0 4px;
  }
  .logo {
    width: 72px;
    height: 72px;
    border-radius: 16px;
    object-fit: contain;
    background: transparent;
  }

  h2 {
    margin: 0;
    font-size: 18px;
    font-weight: 700;
    letter-spacing: -0.01em;
    display: inline-flex;
    align-items: baseline;
    gap: 6px;
    flex-wrap: wrap;
    justify-content: center;
  }
  .version-plain {
    font-size: 12px;
    font-weight: 500;
    color: var(--fg-subtle);
    letter-spacing: 0;
  }

  .compat-label {
    margin: 4px 0 0 0;
    font-size: 12px;
    color: var(--fg-muted);
  }
  .compat-list {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 6px;
  }
  .compat-chip {
    padding: 4px 10px;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: color-mix(in srgb, var(--bg-card) 88%, #fff 12%);
    font-size: 12px;
    font-weight: 600;
  }

  .deal-title {
    text-align: center;
    font-size: 24px;
    font-weight: 900;
    letter-spacing: 0.08em;
    color: #ffffff;
  }

  .deal-gif {
    max-width: 100%;
    max-height: 36vh;
    width: auto;
    height: auto;
    flex: 0 1 auto;
    min-height: 80px;
    object-fit: contain;
    display: block;
    margin: 0 auto;
    border-radius: 10px;
    animation: gifSwap 240ms ease-out;
  }

  .intro {
    margin: 0 auto;
    max-width: 46ch;
    font-size: 12.5px;
    line-height: 1.6;
    color: var(--fg-muted);
    text-align: center;
  }
  .question {
    margin: 2px 0 0;
    font-size: 17px;
    font-weight: 800;
    text-align: center;
    letter-spacing: 0.01em;
  }

  .deal-buttons {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .deal-row {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 3px;
    padding: 11px 16px;
    border-radius: 12px;
    border: 1px solid var(--border);
    background: color-mix(in srgb, var(--bg-card) 88%, #fff 12%);
    color: var(--fg);
    text-align: left;
    cursor: pointer;
    transition: transform 120ms ease-out,
                border-color 160ms ease-out,
                background 160ms ease-out,
                color 160ms ease-out,
                box-shadow 160ms ease-out;
  }
  .deal-row:hover:not(:disabled) {
    transform: translateY(-1px);
    border-color: color-mix(in srgb, var(--fg) 45%, var(--border));
    background: color-mix(in srgb, var(--bg-card) 80%, #fff 20%);
  }
  .deal-row:disabled { opacity: 0.5; cursor: not-allowed; }

  .deal-row-label {
    font-size: 13px;
    font-weight: 700;
    display: inline-flex;
    align-items: baseline;
    gap: 6px;
    flex-wrap: wrap;
  }
  .default-inline {
    font-size: 11px;
    font-weight: 500;
    color: var(--fg-subtle);
  }
  .deal-row-body {
    font-size: 11.5px;
    line-height: 1.5;
    color: var(--fg-muted);
  }

  .no-btn:hover:not(:disabled) {
    background: color-mix(in srgb, #ef4444 14%, var(--bg-card));
    border-color: color-mix(in srgb, #ef4444 55%, var(--border));
    color: #ef4444;
  }
  .no-btn:hover:not(:disabled) .deal-row-body {
    color: #ef4444;
  }
  .no-btn.no-clicked,
  .no-btn.no-clicked:disabled {
    background: #ef4444 !important;
    border-color: #ef4444 !important;
    color: #ffffff !important;
    opacity: 1 !important;
    box-shadow: 0 10px 28px color-mix(in srgb, #ef4444 35%, transparent);
  }
  .no-btn.no-clicked .deal-row-body {
    color: #ffffff !important;
  }

  .deal-row.deal-accent {
    border-color: rgba(255, 255, 255, 0.65);
    background: color-mix(in srgb, var(--bg-card) 86%, #fff 14%);
    box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.18),
                0 6px 22px rgba(255, 255, 255, 0.08);
  }
  .deal-row.deal-accent .deal-row-label {
    color: #ffffff;
    letter-spacing: 0.04em;
  }
  .deal-row.deal-accent:hover:not(:disabled) {
    transform: translateY(-2px);
    border-color: #ffffff;
    background: color-mix(in srgb, var(--bg-card) 78%, #fff 22%);
    box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.45),
                0 14px 32px rgba(255, 255, 255, 0.16);
  }

  .actions {
    margin-top: 4px;
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    align-items: center;
  }
  .actions.split { justify-content: space-between; align-items: center; }

  .primary {
    border: none;
    border-radius: 10px;
    padding: 10px 18px;
    font-size: 13px;
    font-weight: 700;
    background: var(--fg);
    color: var(--bg-solid);
    cursor: pointer;
    transition: opacity 120ms ease-out, transform 120ms ease-out;
  }
  .primary:hover:not(:disabled) { opacity: 0.9; transform: translateY(-1px); }
  .primary:disabled { opacity: 0.5; cursor: not-allowed; }

  .ghost {
    border: 1px solid var(--border);
    background: transparent;
    color: var(--fg-muted);
    border-radius: 10px;
    padding: 9px 14px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: color 120ms ease-out, border-color 120ms ease-out;
  }
  .ghost:hover:not(:disabled) {
    color: var(--fg);
    border-color: color-mix(in srgb, var(--fg) 35%, var(--border));
  }
  .ghost:disabled { opacity: 0.5; cursor: not-allowed; }

  @keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }
  @keyframes modalIn {
    from { opacity: 0; transform: scale(0.96) translateY(8px); }
    to { opacity: 1; transform: scale(1) translateY(0); }
  }
  @keyframes dockIn {
    from { opacity: 0; transform: translate(-50%, 14px) scale(0.97); }
    to { opacity: 1; transform: translate(-50%, 0) scale(1); }
  }
  @keyframes stepIn {
    from { opacity: 0; transform: translateY(6px); }
    to { opacity: 1; transform: translateY(0); }
  }
  @keyframes featureSwap {
    from { opacity: 0; transform: translateY(4px); }
    to { opacity: 1; transform: translateY(0); }
  }
  @keyframes ctxIn {
    from { opacity: 0; transform: translateX(-6px) scale(0.97); }
    to { opacity: 1; transform: translateX(0) scale(1); }
  }
  @keyframes pulse {
    0%, 100% { filter: brightness(1); }
    50% { filter: brightness(1.25); }
  }
  @keyframes gifSwap {
    from { opacity: 0; transform: scale(0.96); }
    to { opacity: 1; transform: scale(1); }
  }

  :global(html[data-motion="reduced"]) :is(.backdrop, .step, .spotlight, .modal, .modal.dock-bottom, .feature-bare, .mock-ctx-menu, .deal-gif) { animation: none; }
  :global(html[data-motion="reduced"]) :is(.deal-row:hover:not(:disabled), .primary:hover:not(:disabled)) { transform: none; }
</style>
