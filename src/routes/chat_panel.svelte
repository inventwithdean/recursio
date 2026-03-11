<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import { onMount } from "svelte";
    import { goto } from "$app/navigation";
    interface Chat {
        id: string;
        title: string;
    }

    let {
        onNewChat,
        onLoadChat,
    }: {
        onNewChat: () => void;
        onLoadChat: (id: string) => void;
    } = $props();

    let expanded = $state(false);
    let chats = $state<Chat[]>([]);

    async function loadChats() {
        chats = await invoke<Chat[]>("get_chats"); // Returns Vec<String, String>
    }

    async function handleNewChat() {
        await invoke("new_conversation");
        onNewChat();
        await loadChats();
    }

    async function handleLoadChat(id: string) {
        console.log("Trying to load", id);
        onLoadChat(id);
    }

    $effect(() => {
        if (expanded) loadChats();
    });

    onMount(() => {
        loadChats();
    });
</script>

<aside
    class="
    h-screen flex flex-col shrink-0
    bg-bg border-r border-border
    transition-[width] duration-300 ease-out
    overflow-hidden
    {expanded ? 'w-64' : 'w-12'}
"
>
    <!-- Toggle Button -->
    <div
        class="flex items-center justify-between px-3 py-4 shrink-0 border-b border-border"
    >
        {#if expanded}
            <span
                class="text-[14px] font-mono tracking-widest whitespace-nowrap"
                >recursio</span
            >
        {/if}
        <!-- svelte-ignore a11y_consider_explicit_label -->
        <button
            onclick={() => (expanded = !expanded)}
            class="
            w-6 h-6 flex items-center justify-center rounded-md
            text-dim hover:text-text hover:bg-surface
            border border-transparent hover:border-border
            transition-all duration-150 cursor-pointer shrink-0
            "
            ><svg
                width="20"
                height="20"
                viewBox="0 0 20 20"
                fill="currentColor"
                aria-hidden="true"
            >
                <path
                    d="M16.5 4C17.3284 4 18 4.67157 18 5.5V14.5C18 15.3284 17.3284 16 16.5 16H3.5C2.67157 16 2 15.3284 2 14.5V5.5C2 4.67157 2.67157 4 3.5 4H16.5ZM7 15H16.5C16.7761 15 17 14.7761 17 14.5V5.5C17 5.22386 16.7761 5 16.5 5H7V15ZM3.5 5C3.22386 5 3 5.22386 3 5.5V14.5C3 14.7761 3.22386 15 3.5 15H6V5H3.5Z"
                />
            </svg>
        </button>
    </div>
    <!-- svelte-ignore a11y_consider_explicit_label -->
    <div
        class="flex items-center {expanded
            ? 'justify-start px-3'
            : 'justify-center'} px-2 py-2 w-full"
    >
        <button
            onclick={handleNewChat}
            class="
                w-full flex items-center gap-2.5 px-2 py-2.5
                text-dim hover:text-text hover:bg-surface
                border border-transparent hover:border-border rounded-md
                transition-all duration-150 cursor-pointer
                "
        >
            <svg
                width="16"
                height="16"
                viewBox="0 0 20 20"
                fill="currentColor"
                class="text-text-300 group-hover:text-text-000"
                aria-hidden="true"
                style="flex-shrink: 0;"
                ><path
                    d="M10 3C10.4142 3 10.75 3.33579 10.75 3.75V9.25H16.25C16.6642 9.25 17 9.58579 17 10C17 10.3882 16.7051 10.7075 16.3271 10.7461L16.25 10.75H10.75V16.25C10.75 16.6642 10.4142 17 10 17C9.58579 17 9.25 16.6642 9.25 16.25V10.75H3.75C3.33579 10.75 3 10.4142 3 10C3 9.58579 3.33579 9.25 3.75 9.25H9.25V3.75C9.25 3.33579 9.58579 3 10 3Z"
                ></path></svg
            >
            {#if expanded}
                <span class="ml-2 text-sm font-mono text-left whitespace-nowrap"
                    >New Chat</span
                >
            {/if}
        </button>
    </div>
    {#if expanded}
        <div class="px-6 py-2 text-xs text-dim font-mono">
            <span>Recents</span>
        </div>
        <div class="flex-1 overflow-y-auto px-3 pb-4 flex flex-col gap-0.5">
            {#if chats.length === 0}
                <p
                    class="text-xs font-mono text-dim px-2 py-4 text-center whitespace-nowrap"
                >
                    no chats yet
                </p>
            {:else}
                {#each chats as chat}
                    <button
                        onclick={() => handleLoadChat(chat.id)}
                        class="
                            w-full text-left px-3 py-2.5 rounded-lg text-xs font-mono
                            transition-all duration-150 cursor-pointer
                            text-dim hover:text-text hover:bg-surface/50
                            border border-transparent
                            "
                    >
                        <span class="truncate block"
                            >{chat.title || "untitled"}</span
                        >
                    </button>
                {/each}
            {/if}
        </div>
    {/if}
    <div class="mt-auto border-t border-border px-3 py-3">
        <button
            onclick={() => goto("/setup")}
            class="w-full flex items-center gap-2.5 px-2 py-2.5
            text-dim hover:text-text hover:bg-surface
            border border-transparent hover:border-border rounded-md
            transition-all duration-150 cursor-pointer
            "
        >
            {#if expanded}
                <span>Models</span>
            {/if}
        </button>
    </div>
</aside>
