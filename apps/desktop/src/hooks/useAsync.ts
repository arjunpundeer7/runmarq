import type { DependencyList } from "react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { errorMessage } from "../services/native";

export interface AsyncState<T> {
  data: T | null;
  error: string | null;
  loading: boolean;
  reload: () => void;
}

interface Result<T> {
  data: T | null;
  error: string | null;
  loading: boolean;
}

/** Runs an async loader when dependencies change; ignores stale responses. */
export function useAsync<T>(load: () => Promise<T>, deps: DependencyList): AsyncState<T> {
  const [result, setResult] = useState<Result<T>>({ data: null, error: null, loading: true });
  const [nonce, setNonce] = useState(0);
  const loader = useRef(load);
  useLayoutEffect(() => {
    loader.current = load;
  });

  useEffect(() => {
    let cancelled = false;
    Promise.resolve()
      .then(() => {
        if (!cancelled) setResult((r) => (r.loading ? r : { ...r, loading: true }));
        return loader.current();
      })
      .then((data) => {
        if (!cancelled) setResult({ data, error: null, loading: false });
      })
      .catch((e) => {
        if (!cancelled) setResult((r) => ({ ...r, error: errorMessage(e), loading: false }));
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, nonce]);

  const reload = useCallback(() => setNonce((n) => n + 1), []);
  return { ...result, reload };
}
