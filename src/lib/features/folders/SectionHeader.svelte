<script lang="ts">
  // Compatibility shim: the component moved to shared/components so the
  // shared layer stops importing from features/. Existing feature-layer
  // imports keep working through this passthrough.
  import SectionHeader from "$lib/shared/components/SectionHeader.svelte";
  import type { ComponentProps } from "svelte";

  let props: ComponentProps<typeof SectionHeader> = $props();
</script>

<SectionHeader {...props} />
