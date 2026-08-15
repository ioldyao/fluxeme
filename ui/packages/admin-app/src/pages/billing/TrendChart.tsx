import { useMemo } from 'react';

type DataPoint = {
  date?: string;
  total_cost: number;
};

type Props = {
  data: DataPoint[];
  previousData?: DataPoint[];
  height?: number;
  emptyLabel?: string;
};

function buildPath(
  data: DataPoint[],
  width: number,
  height: number,
  padTop: number,
  padBottom: number,
) {
  if (data.length === 0) return '';
  const values = data.map((d) => d.total_cost);
  const max = Math.max(...values, 1);
  const plotH = height - padTop - padBottom;
  return data
    .map((d, i) => {
      const x = data.length === 1 ? width / 2 : (i / (data.length - 1)) * width;
      const y = padTop + plotH - (d.total_cost / max) * plotH;
      return `${i === 0 ? 'M' : 'L'}${x},${y}`;
    })
    .join(' ');
}

function buildAreaPath(line: string, width: number, height: number) {
  if (!line) return '';
  return `${line} L${width},${height} L0,${height} Z`;
}

export function TrendChart({ data, previousData, height = 235, emptyLabel }: Props) {
  const W = 920;
  const H = height;
  const PAD_TOP = 30;
  const PAD_BOTTOM = 10;

  const mainPath = useMemo(
    () => buildPath(data, W, H, PAD_TOP, PAD_BOTTOM),
    [data, W, H, PAD_TOP, PAD_BOTTOM],
  );
  const areaPath = useMemo(
    () => buildAreaPath(mainPath, W, H - PAD_BOTTOM),
    [mainPath, W, H],
  );
  const prevPath = useMemo(
    () =>
      previousData
        ? buildPath(previousData, W, H, PAD_TOP, PAD_BOTTOM)
        : '',
    [previousData, W, H, PAD_TOP, PAD_BOTTOM],
  );

  const values = data.map((d) => d.total_cost);
  const max = Math.max(...values, 1);

  if (data.length === 0) {
    return (
      <div
        className="grid place-items-center text-sm text-[#7b8496]"
        style={{ height }}
      >
        {emptyLabel || '暂无趋势数据'}
      </div>
    );
  }

  return (
    <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" className="h-full w-full">
      <defs>
        <linearGradient id="trend-fill" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#5268f6" stopOpacity="0.2" />
          <stop offset="100%" stopColor="#5268f6" stopOpacity="0" />
        </linearGradient>
      </defs>

      <line x1={45} y1={PAD_TOP} x2={45} y2={H - PAD_BOTTOM} stroke="#edf0f4" />
      <line x1={45} y1={H - PAD_BOTTOM} x2={W - 20} y2={H - PAD_BOTTOM} stroke="#edf0f4" />
      <line x1={45} y1={PAD_TOP + (H - PAD_TOP - PAD_BOTTOM) * 0.66} x2={W - 20} y2={PAD_TOP + (H - PAD_TOP - PAD_BOTTOM) * 0.66} stroke="#f0f2f5" />
      <line x1={45} y1={PAD_TOP + (H - PAD_TOP - PAD_BOTTOM) * 0.33} x2={W - 20} y2={PAD_TOP + (H - PAD_TOP - PAD_BOTTOM) * 0.33} stroke="#f0f2f5" />

      <text x={4} y={H - PAD_BOTTOM + 5} className="fill-[#98a2b3]" fontSize={10}>¥0</text>
      <text x={4} y={PAD_TOP + (H - PAD_TOP - PAD_BOTTOM) * 0.66 + 5} className="fill-[#98a2b3]" fontSize={10}>
        ¥{(max * 0.33).toFixed(0)}
      </text>
      <text x={4} y={PAD_TOP + (H - PAD_TOP - PAD_BOTTOM) * 0.33 + 5} className="fill-[#98a2b3]" fontSize={10}>
        ¥{(max * 0.66).toFixed(0)}
      </text>

      {areaPath ? <path d={areaPath} fill="url(#trend-fill)" /> : null}
      {prevPath ? (
        <path d={prevPath} fill="none" stroke="#b7bec9" strokeWidth={2} strokeDasharray="6 5" strokeLinecap="round" />
      ) : null}
      {mainPath ? (
        <path d={mainPath} fill="none" stroke="#5268f6" strokeWidth={3} strokeLinecap="round" />
      ) : null}
    </svg>
  );
}
