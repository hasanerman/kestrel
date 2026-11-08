const VISIBLE = 120;

export function drawGraph(canvas: HTMLCanvasElement, history: (number | null)[], color: string) {
  const rect = canvas.getBoundingClientRect();
  if (!rect.width) return;
  const ratio = window.devicePixelRatio || 1;
  canvas.width = Math.round(rect.width * ratio);
  canvas.height = Math.round(rect.height * ratio);
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.scale(ratio, ratio);
  ctx.clearRect(0, 0, rect.width, rect.height);

  const points = history.slice(-VISIBLE);
  const answered = points.filter((v): v is number => v !== null);
  if (answered.length < 2) return;

  const low = Math.max(0, Math.min(...answered) - 8);
  const high = Math.max(...answered) + 8;
  const width = rect.width;
  const height = rect.height;
  const step = width / (VISIBLE - 1);
  const offset = (VISIBLE - points.length) * step;
  const y = (v: number) => height - 6 - ((v - low) / (high - low)) * (height - 12);

  const fill = ctx.createLinearGradient(0, 0, 0, height);
  fill.addColorStop(0, `${color}33`);
  fill.addColorStop(1, `${color}00`);

  ctx.lineWidth = 1.6;
  ctx.lineJoin = "round";
  ctx.strokeStyle = color;

  let segment: [number, number][] = [];
  const flush = () => {
    if (segment.length > 1) {
      ctx.beginPath();
      segment.forEach(([px, py], i) => (i ? ctx.lineTo(px, py) : ctx.moveTo(px, py)));
      ctx.stroke();
      ctx.lineTo(segment[segment.length - 1][0], height);
      ctx.lineTo(segment[0][0], height);
      ctx.closePath();
      ctx.fillStyle = fill;
      ctx.fill();
    }
    segment = [];
  };

  points.forEach((value, i) => {
    if (value === null) {
      flush();
      return;
    }
    segment.push([offset + i * step, y(value)]);
  });
  const tail = segment[segment.length - 1];
  flush();

  if (tail) {
    ctx.beginPath();
    ctx.arc(Math.min(tail[0], width - 3), tail[1], 3, 0, Math.PI * 2);
    ctx.fillStyle = color;
    ctx.fill();
  }
}
