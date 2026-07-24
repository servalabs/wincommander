export interface PhysicalRectLike {
  position: { x: number; y: number };
  size: { width: number; height: number };
}

export interface NotificationWindowBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

const DESIRED_WIDTH = 440;
const DESIRED_HEIGHT = 520;
const EDGE_MARGIN = 12;

export function getNotificationWindowBounds(
  workArea: PhysicalRectLike,
  scaleFactor: number,
): NotificationWindowBounds {
  const scale = Number.isFinite(scaleFactor) && scaleFactor > 0 ? scaleFactor : 1;
  const areaWidth = Math.max(1, Math.floor(workArea.size.width));
  const areaHeight = Math.max(1, Math.floor(workArea.size.height));
  const requestedMargin = Math.max(0, Math.round(EDGE_MARGIN * scale));
  const marginX = Math.min(requestedMargin, Math.floor((areaWidth - 1) / 2));
  const marginY = Math.min(requestedMargin, Math.floor((areaHeight - 1) / 2));
  const width = Math.min(Math.round(DESIRED_WIDTH * scale), areaWidth - marginX * 2);
  const height = Math.min(Math.round(DESIRED_HEIGHT * scale), areaHeight - marginY * 2);

  return {
    x: workArea.position.x + areaWidth - width - marginX,
    y: workArea.position.y + areaHeight - height - marginY,
    width,
    height,
  };
}
