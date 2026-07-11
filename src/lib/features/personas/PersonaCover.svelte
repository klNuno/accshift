<script lang="ts">
  import { PLATFORM_ICON_PATHS, PERSONAS_ICON_PATH } from "$lib/shared/platformIcons";

  export interface CoverTile {
    key: string;
    avatarUrl: string | null;
    accent: string;
    platformId: string;
  }

  let {
    image = null,
    tiles = [],
  }: {
    /** User-picked cover (data URL); wins over the mosaic. */
    image?: string | null;
    /** One tile per assigned account, in assignment order. */
    tiles?: CoverTile[];
  } = $props();

  // Every available account contributes to the mosaic: 1 fills the cover,
  // 2 split vertically, 3 = one tall + two stacked, 4+ = 2x2.
  let mosaicTiles = $derived(tiles.slice(0, 4));
</script>

<div class="cover" class:n2={mosaicTiles.length === 2} class:n3={mosaicTiles.length === 3} class:n4={mosaicTiles.length >= 4}>
  {#if image}
    <img class="custom" src={image} alt="" draggable="false" />
  {:else if mosaicTiles.length === 0}
    <div class="tile placeholder">
      <svg width="26" height="26" viewBox="0 0 24 24" fill="currentColor">
        <path d={PERSONAS_ICON_PATH} />
      </svg>
    </div>
  {:else}
    {#each mosaicTiles as tile (tile.key)}
      <div class="tile" style={`--tile-accent:${tile.accent};`}>
        {#if tile.avatarUrl}
          <img src={tile.avatarUrl} alt="" draggable="false" loading="lazy" />
        {:else}
          <svg width="45%" height="45%" viewBox="0 0 24 24" fill="currentColor">
            <path d={PLATFORM_ICON_PATHS[tile.platformId] ?? PERSONAS_ICON_PATH} />
          </svg>
        {/if}
      </div>
    {/each}
  {/if}
</div>

<style>
  .cover {
    width: 100%;
    aspect-ratio: 1 / 1;
    border-radius: 10px;
    overflow: hidden;
    display: grid;
    grid-template-columns: 1fr;
    grid-template-rows: 1fr;
    background: var(--bg-muted);
  }

  .cover.n2 {
    grid-template-columns: 1fr 1fr;
  }

  .cover.n3,
  .cover.n4 {
    grid-template-columns: 1fr 1fr;
    grid-template-rows: 1fr 1fr;
  }

  /* 3 tiles: the first spans the left column, the other two stack right. */
  .cover.n3 .tile:first-child {
    grid-row: span 2;
  }

  .custom {
    grid-column: 1 / -1;
    grid-row: 1 / -1;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .tile {
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 0;
    min-height: 0;
    background: color-mix(in srgb, var(--tile-accent, var(--fg-subtle)) 16%, var(--bg-muted));
    color: color-mix(in srgb, var(--tile-accent, var(--fg-subtle)) 75%, var(--fg-muted));
  }

  .tile img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .tile.placeholder {
    background: var(--bg-muted);
    color: var(--fg-subtle);
  }
</style>
