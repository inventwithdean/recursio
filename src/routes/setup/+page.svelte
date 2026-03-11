<script lang="ts">
    import { onMount, onDestroy } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { listen } from "@tauri-apps/api/event";
    import { goto } from "$app/navigation";

    type ModelStatus = "default" | "downloading" | "downloaded";

    // TODO: In next update, add other models too!
    interface Model {
        id: string;
        display_name: string;
        description: string;
        size_bytes: number;
        vram_gb: number;
        status: ModelStatus;
        progress: number; // 0..1, only relevant when status === "downloading"
    }

    interface ModelBackend {
        id: string;
        display_name: string;
        description: string;
        file_name: string;
        url: string;
        sha256: string;
        size_bytes: number;
        vram_gb: number;
        downloaded: boolean;
    }

    const FEATURED = ["recursio", "recursio_pro", "recursio_ultra"];
    const FEATURED_LABELS: Record<string, string> = {
        recursio: "BASE",
        recursio_pro: "PRO",
        recursio_ultra: "ULTRA",
    };

    let models = $state<Model[]>([]);
    let launching = $state("");
    let error = $state("");

    let featured = $derived(
        models
            .filter((m) => FEATURED.includes(m.id))
            .sort((a, b) => FEATURED.indexOf(a.id) - FEATURED.indexOf(b.id)),
    );
    let others = $derived(models.filter((m) => !FEATURED.includes(m.id)));

    interface DownloadProgressPayload {
        model_id: string;
        bytes_downloaded: number;
        total_bytes: number;
    }
    onMount(async () => {
        const raw = await invoke<ModelBackend[]>("get_models");
        models = raw.map((m) => ({
            id: m.id,
            display_name: m.display_name,
            description: m.description,
            size_bytes: m.size_bytes,
            vram_gb: m.vram_gb,
            status: m.downloaded ? "downloaded" : "default",
            progress: 0,
        }));
    });
    listen<DownloadProgressPayload>("download_progress", (e) => {
        const { model_id, bytes_downloaded, total_bytes } = e.payload;
        update(model_id, {
            progress: total_bytes > 0 ? bytes_downloaded / total_bytes : 0,
        });
    });

    listen<{ model_id: string }>("download_complete", (e) => {
        update(e.payload.model_id, { status: "downloaded", progress: 1 });
    });

    listen<{ model_id: string; error: string }>("download_error", (e) => {
        update(e.payload.model_id, { status: "default", progress: 0 });
        error = `Download failed: ${e.payload.error}`;
    });

    async function download(model: Model) {
        error = "";
        update(model.id, { status: "downloading", progress: 0 });
        try {
            await invoke("download_model", { modelId: model.id });
        } catch (e: any) {
            update(model.id, { status: "default" });
            error = String(e);
        }
    }

    async function launch(model: Model) {
        error = "";
        launching = model.id;
        try {
            await invoke("launch_model", { modelId: model.id });
            goto("/");
        } catch (e: any) {
            launching = "";
            error = String(e);
        }
    }

    function fmt_size(bytes: number): string {
        const gb = bytes / 1_073_741_824;
        return gb >= 1
            ? `${gb.toFixed(1)} GB`
            : `${(bytes / 1_048_576).toFixed(0)} MB`;
    }

    function update(id: string, patch: Partial<Model>) {
        models = models.map((m) => (m.id === id ? { ...m, ...patch } : m));
    }
</script>

{#snippet modelCard(model: Model, featured: boolean)}
    <div
        class="flex flex-col gap-5 p-7 rounded-2xl transition-all duration-200 h-full
    bg-surface
    {model.status === 'downloaded' ? 'border-accent/30' : 'border-border/60'}
    {model.status === 'downloading' ? 'border-accent/40' : ''}
    hover:border-accent/30 hover:bg-surface/80"
    >
        <!-- Top row: tier badge + ready indicator -->
        {#if featured}
            <div class="flex items-center justify-between">
                <span
                    class="font-mono text-[10px] tracking-[0.2em] px-2.5 py-1 rounded-md
         {featured
                        ? 'bg-accent/10 text-accent/80 border border-accent/20'
                        : 'bg-white/5 text-dim border border-border'}"
                >
                    {featured ? FEATURED_LABELS[model.id] : ""}
                </span>
            </div>
        {/if}

        <!-- Name + description -->
        <div class="flex flex-col gap-2">
            <h2
                class="font-mono font-bold text-text tracking-tight
        {featured ? 'text-xl' : 'text-base'}"
            >
                {model.display_name}
            </h2>
            <p class="font-mono text-xs text-dim leading-relaxed">
                {model.description}
            </p>
        </div>

        <!-- Specs -->
        <div
            class="flex items-center gap-4 font-mono border-t border-border/40 pt-4"
        >
            <div class="flex flex-col gap-1">
                <span class="text-[9px] tracking-[0.15em] text-dim uppercase"
                    >Size</span
                >
                <span class="text-sm text-text font-medium"
                    >{fmt_size(model.size_bytes)}</span
                >
            </div>
            <div class="w-px h-6 bg-border/40"></div>
            <div class="flex flex-col gap-1">
                <span class="text-[9px] tracking-[0.15em] text-dim uppercase"
                    >VRAM</span
                >
                <span class="text-sm text-text font-medium"
                    >≥ {model.vram_gb} GB</span
                >
            </div>
        </div>

        <!-- Progress bar — only when downloading, same width as button -->
        {#if model.status === "downloading"}
            <div class="flex flex-col gap-1">
                <span class="text-xs text-dim font-mono">Downloading...</span>
                <div class="flex items-center gap-3 w-full">
                    <div
                        class="flex-1 h-[2px] bg-white/5 rounded-full overflow-hidden"
                    >
                        <div
                            class="h-full bg-accent/80 rounded-full transition-all duration-300"
                            style="width: {(model.progress * 100).toFixed(1)}%"
                        ></div>
                    </div>
                    <span
                        class="font-mono text-xs text-accent/70 tabular-nums w-9 text-right"
                    >
                        {(model.progress * 100).toFixed(0)}%
                    </span>
                </div>
            </div>
        {/if}

        <!-- Action button -->
        {#if model.status === "downloaded"}
            <button
                onclick={() => launch(model)}
                disabled={launching !== ""}
                class="w-full py-3 rounded-xl font-mono text-xs tracking-widest uppercase
          bg-accent/20 text-accent border border-accent/30
          hover:bg-accent/30 hover:border-accent/40
          transition-all duration-150 cursor-pointer
          disabled:opacity-25 disabled:pointer-events-none"
            >
                {launching === model.id ? "Launching..." : "Launch"}
            </button>
        {:else if model.status === "default"}
            <button
                onclick={() => download(model)}
                class="w-full py-3 rounded-xl font-mono text-xs tracking-widest uppercase
          bg-accent/10 text-accent/80 border border-accent/15
          hover:bg-accent/20 hover:text-accent hover:border-accent/30
          transition-all duration-150 cursor-pointer
          disabled:opacity-25 disabled:pointer-events-none"
            >
                Download
            </button>
        {/if}
    </div>
{/snippet}

<div
    class="flex flex-col items-center justify-center min-h-screen bg-bg px-8 py-20 gap-14"
>
    <!-- Header -->
    <div class="flex flex-col items-center gap-2 text-center">
        <h1 class="font-mono font-bold text-4xl text-text tracking-tight">
            recursio
        </h1>
        <p class="font-mono font-bold text-sm text-dim">
            private ai with internet access
        </p>
    </div>
    {#if error}
        <div
            class="flex items-center justify-between gap-4 w-full max-w-4xl px-5 py-3.5
      border border-red-500/20 rounded-xl font-mono text-xs text-red-400/80"
        >
            <span>⚠ {error}</span>
            <button
                onclick={() => (error = "")}
                class="opacity-40 hover:opacity-100 transition-opacity cursor-pointer"
                >✕</button
            >
        </div>
    {/if}
    <!-- Featured models -->
    <div class="flex flex-col gap-3 w-full max-w-4xl">
        <!-- <p class="font-mono text-[10px] tracking-[0.2em] text-dim uppercase">
            Official
        </p> -->
        <div class="grid grid-cols-3 gap-4">
            {#each featured as model}
                {@render modelCard(model, true)}
            {/each}
        </div>
    </div>
    {#if others.length > 0}
        <div class="flex flex-col gap-4 w-full max-w-4xl">
            <p
                class="font-mono text-[10px] tracking-[0.2em] text-dim uppercase"
            >
                Community
            </p>
            <div class="grid grid-cols-3 gap-4">
                {#each others as model}
                    {@render modelCard(model, false)}
                {/each}
            </div>
        </div>
    {/if}
</div>
