import { useEffect, useRef, type ComponentPropsWithoutRef } from 'react';

function pinchPoint(touches: TouchList) {
  const first = touches[0];
  const second = touches[1];
  return {
    x: (first.clientX + second.clientX) / 2,
    y: (first.clientY + second.clientY) / 2,
    distance: Math.max(1, Math.hypot(first.clientX - second.clientX, first.clientY - second.clientY)),
  };
}

export function PreviewZoom({
  children, className = '', media = false, ...props
}: ComponentPropsWithoutRef<'div'> & { media?: boolean }) {
  const viewport = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const area = viewport.current!;
    const page = content.current!;
    const minimumScale = media ? 1 : 0.5;
    let scale = 1;
    let gesture: { distance: number; scale: number; x: number; y: number } | undefined;
    let width = 0;
    let height = 0;

    const reset = () => {
      scale = 1;
      gesture = undefined;
      page.style.removeProperty('zoom');
      page.style.removeProperty('width');
      page.style.removeProperty('height');
    };
    const start = (event: TouchEvent) => {
      if (event.touches.length !== 2) {
        gesture = undefined;
        return;
      }
      if (event.cancelable) event.preventDefault();
      const point = pinchPoint(event.touches);
      const rect = page.getBoundingClientRect();
      if (scale === 1) {
        width = page.offsetWidth;
        height = page.offsetHeight;
      }
      gesture = {
        distance: point.distance,
        scale,
        x: (point.x - rect.left) / scale,
        y: (point.y - rect.top) / scale,
      };
    };
    const move = (event: TouchEvent) => {
      if (event.touches.length !== 2 || !gesture) return;
      if (event.cancelable) event.preventDefault();
      const point = pinchPoint(event.touches);
      scale = Math.max(minimumScale, Math.min(4, gesture.scale * point.distance / gesture.distance));
      page.style.width = scale === 1 ? '' : width + 'px';
      if (media) page.style.height = scale === 1 ? '' : height + 'px';
      page.style.zoom = scale === 1 ? '' : String(scale);
      const rect = page.getBoundingClientRect();
      area.scrollLeft += rect.left + gesture.x * scale - point.x;
      area.scrollTop += rect.top + gesture.y * scale - point.y;
    };
    const end = () => { gesture = undefined; };
    let areaWidth = area.offsetWidth;
    let areaHeight = area.offsetHeight;
    const observer = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(() => {
      if (area.offsetWidth === areaWidth && area.offsetHeight === areaHeight) return;
      areaWidth = area.offsetWidth;
      areaHeight = area.offsetHeight;
      reset();
    }) : null;
    observer?.observe(area, { box: 'border-box' });
    area.addEventListener('touchstart', start, { capture: true, passive: false });
    area.addEventListener('touchmove', move, { capture: true, passive: false });
    area.addEventListener('touchend', end, true);
    area.addEventListener('touchcancel', end, true);
    return () => {
      observer?.disconnect();
      area.removeEventListener('touchstart', start, true);
      area.removeEventListener('touchmove', move, true);
      area.removeEventListener('touchend', end, true);
      area.removeEventListener('touchcancel', end, true);
      reset();
    };
  }, [media]);

  return (
    <div {...props} className={className + ' preview-zoom'} ref={viewport}>
      <div className={media ? 'preview-media-content' : undefined} ref={content}>{children}</div>
    </div>
  );
}
