import { useState, useEffect, RefObject } from "react";

export function useDashboardScale(ref: RefObject<HTMLElement | null>) {
  const [scale, setScale] = useState(1);

  useEffect(() => {
    if (!ref.current) return;
    
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const { width, height } = entry.contentRect;
        if (width === 0 || height === 0) continue;
        
        // Minimum required dimensions to fit all absolute/flex elements without overlap.
        // Left bar (240) + Right bar (280) + Radar/Matrix (~500) + label overflow paddings = ~1280px
        // Top section (180) + Radar (480) + paddings = ~680px
        const minWidth = 1150;
        const minHeight = 680;
        
        const widthScale = width < minWidth ? width / minWidth : 1;
        const heightScale = height < minHeight ? height / minHeight : 1;
        
        let newScale = Math.min(widthScale, heightScale);
        
        // Ensure scale is not larger than 1, and not impossibly small
        newScale = Math.max(0.4, Math.min(1, newScale));
        
        setScale(newScale);
      }
    });
    
    observer.observe(ref.current);
    return () => observer.disconnect();
  }, [ref]);

  return scale;
}
