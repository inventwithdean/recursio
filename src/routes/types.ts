export type Role = "user" | "assistant" | "typing" | "search";
export type ResultStatus = "pending" | "active" | "visited" | "skipped" | "failed";

export interface SearchResult {
    id: string,
    title: string;
    url: string;
    domain: string;
    status: ResultStatus;
}

export interface SearchStatus {
    query: string;
    results: SearchResult[];
}

export interface Message {
    id: string;
    role: Role;
    content: string;
    searchData?: SearchStatus; // only when role is search
}

export interface SearchQueryPayload {
    query: string,
    search_id: string // will be stored in Message.id
}

// Payloads that will be received from the backend
export interface SearchResultsPayload {
    search_results: {
        id: string, // UUID string, will be stored in SearchResult.id
        url: string,
        title: string,
        snippet: string,
    }[]
    query: string,
    search_id: string
}

export interface SearchResultStatusChangePayload {
    search_id: string,
    result_id: string,
    status: string
}