# Rewrite preserved audio references

Warning that preserved cue/M3U/M3U8 references may become stale is insufficient
as a long-term result: knowingly renaming tracks while leaving those files
broken can make the groomed album worse than its source. After v0.1, investigate
safe reference rewriting for understood formats and explicit fallback behavior
when a reference file cannot be updated confidently. Preserve original source
bytes and keep every proposed rewrite visible in preview.
