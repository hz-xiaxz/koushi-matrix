import { useCallback, useEffect, useRef } from "react";

export function useStableEvent<T extends (...args: any[]) => unknown>(handler: T): T {
  const handlerRef = useRef(handler);

  useEffect(() => {
    handlerRef.current = handler;
  }, [handler]);

  return useCallback(((...args: any[]) => handlerRef.current(...args)) as T, []);
}
