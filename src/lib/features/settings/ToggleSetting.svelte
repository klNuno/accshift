<script lang="ts">
  let {
    label,
    enabled = false,
    accent = "#3b82f6",
    onLabel = "On",
    offLabel = "Off",
    onToggle,
  }: {
    label: string;
    enabled: boolean;
    accent?: string;
    onLabel?: string;
    offLabel?: string;
    onToggle: () => void;
  } = $props();
</script>

<button
  class="toggle-btn"
  type="button"
  style={`--chip-accent:${accent};`}
  onclick={onToggle}
>
  <span class="label">{label}</span>
  <div class="right">
    <strong>{enabled ? onLabel : offLabel}</strong>
    <div class="toggle" class:active={enabled}>
      <div class="knob"></div>
    </div>
  </div>
</button>

<style>
  .toggle-btn {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: color-mix(in srgb, var(--bg) 88%, #fff 12%);
    color: var(--fg);
    padding: 9px 10px;
    cursor: pointer;
    transition: border-color 120ms ease-out, background 120ms ease-out;
    font-size: 12px;
  }

  .label {
    min-width: 0;
    text-align: left;
  }

  .right {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-shrink: 0;
  }

  .right strong {
    font-size: 12px;
    color: var(--fg);
    font-weight: 600;
  }

  .toggle-btn:hover {
    border-color: color-mix(in srgb, var(--chip-accent) 55%, var(--border));
    background: color-mix(in srgb, var(--bg-card) 84%, #fff 16%);
  }

  .toggle {
    width: 36px;
    height: 20px;
    border-radius: 999px;
    background: var(--bg-elevated);
    padding: 2px;
    transition: background 120ms ease-out;
  }

  .toggle.active {
    background: var(--chip-accent);
  }

  .knob {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    transition: transform 120ms ease-out;
  }

  .toggle.active .knob {
    transform: translateX(16px);
  }
</style>
