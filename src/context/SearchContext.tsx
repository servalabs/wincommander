import { createContext, useContext, useState, useCallback, type ReactNode } from "react";

interface SearchContextValue {
    searchQuery: string;
    setSearchQuery: (q: string) => void;
    clearSearch: () => void;
}

const SearchContext = createContext<SearchContextValue>({
    searchQuery: "",
    setSearchQuery: () => {},
    clearSearch: () => {},
});

export function SearchProvider({ children }: { children: ReactNode }) {
    const [searchQuery, setSearchQuery] = useState("");
    const clearSearch = useCallback(() => setSearchQuery(""), []);
    return (
        <SearchContext.Provider value={{ searchQuery, setSearchQuery, clearSearch }}>
            {children}
        </SearchContext.Provider>
    );
}

export function useSearchQuery() {
    return useContext(SearchContext);
}
