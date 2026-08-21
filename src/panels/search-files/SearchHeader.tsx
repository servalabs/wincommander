// src/panels/search-files/SearchHeader.tsx
//
// Panel title block (standard PanelHeader + contextual "?") with the
// quick-search hotkey chip on the right. Pure renderer — hotkey state
// comes from useSearchHotkey.

import PanelHeader from "../../components/shared/PanelHeader";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { SearchHotkeyState } from "@/hooks/useSearchHotkey";

export default function SearchHeader({ hotkey }: { hotkey: SearchHotkeyState }) {
  return (
    <div className="sfp-header">
      <PanelHeader
        title="File Search"
        description="One search box for file names everywhere and the text inside your files."
        panelId="search-files"
      />
      <div className="search-files-hotkey-row">
        <span className="search-files-hotkey-label">Search from anywhere</span>
        {hotkey.recording ? (
          <input
            autoFocus
            className="search-hk-input"
            aria-label="New search hotkey"
            placeholder="Press keys…"
            onKeyDown={hotkey.onRecordKeyDown}
            onBlur={hotkey.stopRecording}
            readOnly
          />
        ) : (
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                className="search-hk-badge"
                aria-label={`Change search hotkey, currently ${hotkey.hotkey || "not set"}`}
                onClick={hotkey.startRecording}
              >
                {hotkey.hotkey}
              </button>
            </TooltipTrigger>
            <TooltipContent side="bottom">
              Opens quick search from any app — click to change the keys
            </TooltipContent>
          </Tooltip>
        )}
      </div>
    </div>
  );
}
