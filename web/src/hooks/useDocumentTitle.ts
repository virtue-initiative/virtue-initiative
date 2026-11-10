import { useEffect } from 'preact/hooks';

export const TITLE_SUFFIX = ' - The Virtue Initiative';

/** Names the page in the browser tab (WEB-002). */
export function useDocumentTitle(label: string | null | undefined) {
  useEffect(() => {
    const trimmed = label?.trim();
    if (trimmed) document.title = `${trimmed}${TITLE_SUFFIX}`;
  }, [label]);
}
