
import { listen } from "@tauri-apps/api/event";
import type { SearchResultsPayload, Message, SearchResult, ResultStatus, SearchResultStatusChangePayload, SearchQueryPayload } from "./types";
import { invoke } from "@tauri-apps/api/core";

function getDomain(url: string): string {
    try {
        return new URL(url).hostname;
    } catch {
        return "";
    }
}

let messages = $state<Message[]>([]);
let isInferenceRunning = $state(false);
let streamingMessage = $state("");

export async function send_message(user_prompt: string) {
    isInferenceRunning = true;
    messages.push({
        id: crypto.randomUUID(),
        role: "user",
        content: user_prompt
    });
    await invoke("send_message", { query: user_prompt });
    isInferenceRunning = false;
}

listen<SearchQueryPayload>("search_query", (event) => {
    console.log("search_query...", event.payload);
    let query = event.payload.query;
    let search_id = event.payload.search_id;
    messages.push({
        id: search_id,
        role: "search",
        content: "",
        searchData: { query: query, results: [] }
    })
})

listen<SearchResultsPayload>("search_results", (event) => {
    let search_results: SearchResult[] = event.payload.search_results.map(sr => ({
        id: sr.id,
        title: sr.title,
        url: sr.url,
        domain: getDomain(sr.url),
        status: "skipped"
    }));
    let search_id = event.payload.search_id;
    let query = event.payload.query;
    // Update messages
    messages = messages.map(msg =>
        msg.id === search_id
            ? { ...msg, searchData: { query, results: search_results } }
            : msg
    );
});


listen<SearchResultStatusChangePayload>("search_result_status_change", (event) => {
    console.log(event.payload);
    let search_id = event.payload.search_id;
    let result_id = event.payload.result_id;
    let status = event.payload.status;

    messages = messages.map(m => {
        if (m.id != search_id || !m.searchData) return m;
        return {
            ...m,
            searchData: {
                ...m.searchData,
                results: m.searchData.results.map(r =>
                    r.id === result_id ? { ...r, status: status as ResultStatus } : r
                )
            }
        };
    });
});

listen<string>("assistant_message", (event) => {
    streamingMessage = "";
    const last = messages[messages.length - 1];
    if (last?.id === "streaming") {
        last.id = crypto.randomUUID();
    }
    invoke("save_conversation", { uiMessages: JSON.stringify(messages) });
});

listen<string>("assistant_chunk", (event) => {
    const last = messages[messages.length - 1];
    if (!last || last.id !== "streaming") {
        streamingMessage = "";
        messages.push({
            id: "streaming",
            role: "assistant",
            content: "",
        });
    }
    streamingMessage += event.payload;
    messages[messages.length - 1].content = streamingMessage;
});

export const chat = {
    get messages() { return messages; },
    get isInferenceRunning() { return isInferenceRunning; },
}

export function onNewChat() {
    messages = []
}

export async function onLoadChat(id: string) {
    const uiMessages = await invoke<string>("load_conversation", { conversationId: id });
    messages = JSON.parse(uiMessages);
}