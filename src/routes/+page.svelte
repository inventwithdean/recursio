<script lang="ts">
  import { onMount, tick } from "svelte";
  import { chat, onLoadChat, onNewChat, send_message } from "./chat.svelte";
  import DOMPurify from "dompurify";
  import { marked } from "marked";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import ChatPanel from "./chat_panel.svelte";

  let user_prompt = $state("");
  let chatEl: HTMLElement;
  let isAtBottom = true;
  let textarea: HTMLTextAreaElement;

  async function scrollToBottom() {
    await tick();
    chatEl?.scrollTo({ top: chatEl.scrollHeight, behavior: "smooth" });
  }

  $effect(() => {
    chat.messages.length; // track length
    scrollToBottom();
  });

  $effect(() => {
    chat.messages[chat.messages.length - 1]?.content.length; // track streaming
    if (isAtBottom) scrollToBottom();
  });

  onMount(() => {
    chatEl.addEventListener("scroll", () => {
      const threshold = 40; // px from bottom
      isAtBottom =
        chatEl.scrollHeight - chatEl.scrollTop - chatEl.clientHeight <
        threshold;
    });
  });

  async function send_user_prompt() {
    let input = user_prompt.trim();
    if (!input) {
      return;
    }
    user_prompt = "";
    await tick();
    autoresize_inputarea();
    textarea.blur();
    await send_message(input);
    await scrollToBottom();
  }

  function autoresize_inputarea() {
    textarea.style.height = "auto";
    textarea.style.height = textarea.scrollHeight + "px";
    textarea.style.overflowY =
      textarea.scrollHeight > textarea.offsetHeight ? "auto" : "hidden";
  }

  function favicon(domain: string): string {
    // return `https://${domain}/favicon.ico`;
    return `https://www.google.com/s2/favicons?domain=${domain}&sz=16`;
  }

  function render(content: string) {
    const html = marked(content) as string;
    return DOMPurify.sanitize(html, {
      ADD_TAGS: ["math", "annotation"],
      ADD_ATTR: ["encoding"],
    });
  }
</script>

<div class="flex flex-row h-screen bg-bg text-text">
  <ChatPanel {onNewChat} {onLoadChat} />
  <div class="flex flex-col flex-1">
    <!-- HEADER -->
    <header class="border-b border-border px-6 py-4 shrink-0">
      <p></p>
    </header>

    <!-- MESSAGES -->
    <main bind:this={chatEl} class="flex-1 overflow-y-auto overflow-x-hidden">
      <!-- Limit the width -->
      <div class="max-w-2xl mx-auto px-6 py-4 flex flex-col gap-4">
        {#each chat.messages as msg}
          <!-- Individual Message, row flex, so justify -->
          <div
            class="flex {msg.role === 'user' ? 'justify-end' : 'justify-start'}"
          >
            <div class="flex flex-col gap-1">
              <!-- role tag -->
              {#if msg.role === "typing"}
                <div
                  class="bg-surface border border-border rounded-2xl rounded-bl-sm px-5 py-4 flex items-center gap-3"
                >
                  <span class="moon">🔭</span>
                  <span class="text-xs font-mono text-dim">thinking...</span>
                </div>
              {:else if msg.role === "search"}
                <div
                  class="max-w-xl w-full bg-surface/50 border border-border/50 rounded-xl overflow-hidden"
                >
                  <div
                    class="flex items-center justify-between px-4 py-2 border-b border-border/50"
                  >
                    <div class="flex items-center gap-2">
                      <span class="text-sm searching-icon">🔍</span>
                      <span
                        class="text-xs font-mono text-text truncate max-w-48"
                      >
                        {msg.searchData?.query}
                      </span>
                    </div>
                    {#if msg.searchData?.results && msg.searchData?.results.length > 0}
                      <span class="text-xs font-mono text-dim shrink-0">
                        {msg.searchData?.results.length} results
                      </span>
                    {/if}
                  </div>
                  <div class="flex flex-col">
                    {#each msg.searchData?.results ?? [] as result}
                      <div
                        class="
                    flex items-center gap-3 px-4 py-2 border-b border-border/30 last:border-0
                    transition-colors duration-300
                    {result.status === 'active' ? 'bg-accent/5' : ''}
                    {result.status === 'visited' ? 'opacity-80' : ''}
                    {result.status === 'skipped' ? 'opacity-60' : ''}
                  "
                      >
                        <!-- Status Indicator -->
                        <div
                          class="shrink-0 w-3 flex items-center justify-center"
                        >
                          {#if result.status === "pending"}
                            <div
                              class="w-1.5 h-1.5 rounded-full bg-border"
                            ></div>
                          {:else if result.status === "active"}
                            <div
                              class="w-1.5 h-1.5 rounded-full bg-accent animate-pulse"
                            ></div>
                          {:else if result.status === "visited"}
                            <div
                              class="w-1.5 h-1.5 rounded-full bg-green-500/70"
                            ></div>
                          {:else if result.status === "failed"}
                            <div
                              class="w-1.5 h-1.5 rounded-full bg-red-500/70"
                            ></div>
                          {:else if result.status === "skipped"}
                            <div
                              class="w-1.5 h-1.5 rounded-full bg-border/30"
                            ></div>
                          {/if}
                        </div>
                        <img
                          src={favicon(result.domain)}
                          alt=""
                          width="14"
                          height="14"
                          class="shrink-0 opacity-70"
                        />
                        <button
                          onclick={() => openUrl(result.url)}
                          class="flex-1 min-w-0 text-left"
                        >
                          <span
                            class="text-xs pr-4 truncate block cursor-pointer opacity-60 hover:opacity-100 transition-opacity duration-150"
                            >{result.title}</span
                          >
                        </button>
                        <button
                          onclick={() => openUrl(`https://${result.domain}`)}
                        >
                          <span
                            class="ml-auto text-xs pl-3 cursor-pointer opacity-40 hover:opacity-90 shrink-0 font-mono shrink-0"
                            >{result.domain}
                          </span>
                        </button>
                      </div>
                    {/each}
                  </div>
                </div>
              {:else}
                <p class="text-dim font-mono text-xs px-1">
                  {msg.role === "user" ? "you" : "agent"}
                </p>
                <!-- message content -->
                <div
                  class="
            px-4 py-3 rounded-2xl text-[16px] leading-relaxed prose prose-invert
            {msg.role === 'user'
                    ? 'bg-accent/10 text-text rounded-br-sm'
                    : 'bg-surface text-text rounded-bl-sm'}
            "
                >
                  {@html render(msg.content)}
                </div>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    </main>

    <!-- INPUT -->
    <footer class="shrink-0">
      <div
        class="max-w-2xl mx-auto py-4 justify-center flex gap-3 items-center"
      >
        <textarea
          bind:this={textarea}
          bind:value={user_prompt}
          oninput={autoresize_inputarea}
          onkeydown={async (e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              send_user_prompt();
            }
          }}
          rows="1"
          placeholder="Ask anything..."
          class="
          flex-1 resize-none bg-surface border border-border/10 rounded-xl
          px-4 py-3 text-sm text-text placeholder:text-dim outline-none
          hover:border-accent/10 focus:border-accent/10
          transition-colors duration-200
          max-h-36
          "
        ></textarea>
        <!-- svelte-ignore a11y_consider_explicit_label -->
        <button
          onclick={send_user_prompt}
          disabled={chat.isInferenceRunning || !user_prompt.trim()}
          class="
          shrink-0 w-10 h-10 rounded-xl bg-accent/20 border border-accent/10
          text-accent flex items-center justify-center
          hover:bg-accent/30 transition-colors duration-200
          cursor-pointer
          disabled:opacity-30 disabled:cursor-not-allowed
        "
        >
          <svg width="16" height="16" viewBox="0 0 256 256" fill="currentColor">
            <path
              d="M208.49,120.49a12,12,0,0,1-17,0L140,69V216a12,12,0,0,1-24,0V69L64.49,120.49a12,12,0,0,1-17-17l72-72a12,12,0,0,1,17,0l72,72A12,12,0,0,1,208.49,120.49Z"
            />
          </svg>
        </button>
      </div>
    </footer>
  </div>
</div>

<style>
  .moon {
    font-size: 18px;
    animation: moonpulse 2s ease-in-out infinite;
    display: inline-block;
    filter: drop-shadow(0 0 0px #a78bfa);
  }

  @keyframes moonpulse {
    0%,
    100% {
      transform: scale(1) rotate(-10deg);
      filter: drop-shadow(0 0 2px #a78bfa) drop-shadow(0 0 6px #a78bfa);
      opacity: 0.7;
    }
    50% {
      transform: scale(1.2) rotate(10deg);
      filter: drop-shadow(0 0 6px #a78bfa) drop-shadow(0 0 14px #a78bfa)
        drop-shadow(0 0 24px #6366f1);
    }
  }
</style>
