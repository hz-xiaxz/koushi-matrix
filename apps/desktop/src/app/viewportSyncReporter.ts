import type { DisplayDensity } from "../domain/types";
import type {
  DesktopApi,
  ViewportSyncObservation,
  ViewportSyncReceipt,
  ViewportSyncTrigger
} from "../backend/desktopApi";

export type FrontendViewportSyncTrigger = Extract<
  ViewportSyncTrigger,
  "density_commit" | "browser_resize"
>;

function finite(value: number): boolean {
  return Number.isFinite(value);
}

function measureSize(width: number, height: number) {
  return finite(width) && finite(height) && width > 0 && height > 0
    ? { width, height }
    : null;
}

function measureRect(element: Element) {
  const { top, left, width, height } = element.getBoundingClientRect();
  return [top, left, width, height].every(finite) && width > 0 && height > 0
    ? { top, left, width, height }
    : null;
}

function measureObservation(
  trigger: FrontendViewportSyncTrigger,
  density: DisplayDensity
): ViewportSyncObservation | null {
  const windowSize = measureSize(window.innerWidth, window.innerHeight);
  const documentSize = measureSize(
    document.documentElement.clientWidth,
    document.documentElement.clientHeight
  );
  const body = document.body ? measureRect(document.body) : null;
  const root = document.querySelector<HTMLElement>(".desktop");
  const rootRect = root ? measureRect(root) : null;
  if (!windowSize || !documentSize || !body || !rootRect) {
    return null;
  }

  const visualViewport = window.visualViewport;
  if (!visualViewport) {
    return {
      trigger,
      density,
      window: windowSize,
      document: documentSize,
      visualViewport: {
        present: false,
        width: 0,
        height: 0,
        offsetLeft: 0,
        offsetTop: 0
      },
      body,
      root: rootRect
    };
  }

  if (
    ![
      visualViewport.width,
      visualViewport.height,
      visualViewport.offsetLeft,
      visualViewport.offsetTop
    ].every(finite) ||
    visualViewport.width <= 0 ||
    visualViewport.height <= 0
  ) {
    return null;
  }

  return {
    trigger,
    density,
    window: windowSize,
    document: documentSize,
    visualViewport: {
      present: true,
      width: visualViewport.width,
      height: visualViewport.height,
      offsetLeft: visualViewport.offsetLeft,
      offsetTop: visualViewport.offsetTop
    },
    body,
    root: rootRect
  };
}

export function createViewportSyncReporter(
  api: Pick<DesktopApi, "observeViewportSync">
): (
  trigger: FrontendViewportSyncTrigger,
  density: DisplayDensity
) => Promise<ViewportSyncReceipt | null> {
  return async (trigger, density) => {
    const observation = measureObservation(trigger, density);
    if (!observation) {
      return null;
    }
    try {
      return await api.observeViewportSync(observation);
    } catch {
      return null;
    }
  };
}
