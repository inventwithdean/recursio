<script>
  import { goto } from "$app/navigation";
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  let { children } = $props();
  import "../app.css";
  import { getCurrentWindow } from "@tauri-apps/api/window";


  onMount(async () => {
    console.log("onMount fired")
    await getCurrentWindow().show();
    // TODO: Check if a model is downloaded, launch directly if it is.
    const modelReady = null;
    if (!modelReady) goto("/setup");
  });

</script>

{@render children()}
