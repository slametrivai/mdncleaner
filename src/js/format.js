// Indonesian formatting: decimal comma, thousands dot, "09.14" times.

const KB = 1024;
const MB = KB * 1024;
export const GB = MB * 1024;
export const DAY_MS = 24 * 60 * 60 * 1000;

const numberFormats = new Map();
function decimals(digits) {
  if (!numberFormats.has(digits)) {
    numberFormats.set(
      digits,
      new Intl.NumberFormat("id-ID", { minimumFractionDigits: digits, maximumFractionDigits: digits }),
    );
  }
  return numberFormats.get(digits);
}

export const formatNumber = (n) => decimals(0).format(n);

/** "1,92 GB", "842 MB". `gbDigits` sets the decimals shown for gigabytes. */
export function formatBytes(bytes, gbDigits = 2) {
  if (bytes >= GB) return `${decimals(gbDigits).format(bytes / GB)} GB`;
  if (bytes >= MB) return `${formatNumber(bytes / MB)} MB`;
  if (bytes >= KB) return `${formatNumber(bytes / KB)} KB`;
  return `${formatNumber(bytes)} B`;
}

/** Whole gigabytes, for drive sizes: "412". */
export const formatGb = (bytes) => formatNumber(Math.round(bytes / GB));

const dateFormat = new Intl.DateTimeFormat("id-ID", { day: "numeric", month: "short", year: "numeric" });
const dayMonthFormat = new Intl.DateTimeFormat("id-ID", { day: "numeric", month: "short" });
const monthFormat = new Intl.DateTimeFormat("id-ID", { month: "long", year: "numeric" });
const timeFormat = new Intl.DateTimeFormat("id-ID", { hour: "2-digit", minute: "2-digit", hour12: false });

/** "8 Nov 2026" */
export const formatDate = (ms) => dateFormat.format(new Date(ms));
/** "15 Nov" */
export const formatDayMonth = (ms) => dayMonthFormat.format(new Date(ms));
/** "November 2026" */
export const formatMonth = (ms) => monthFormat.format(new Date(ms));
/** "09.20" */
export const formatTime = (ms) => timeFormat.format(new Date(ms)).replace(":", ".");

function startOfDay(ms) {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/** "hari ini, 09.14", "kemarin, 20.00" or "31 Okt 2026, 20.00". */
export function formatWhen(ms) {
  const days = Math.round((startOfDay(Date.now()) - startOfDay(ms)) / DAY_MS);
  const time = formatTime(ms);
  if (days === 0) return `hari ini, ${time}`;
  if (days === 1) return `kemarin, ${time}`;
  return `${formatDate(ms)}, ${time}`;
}

/** "3 hari lalu", "14 bulan lalu", "2 tahun lalu". */
export function formatAgo(ms) {
  const days = Math.max(0, Math.floor((Date.now() - ms) / DAY_MS));
  if (days < 30) return `${days} hari lalu`;
  const months = Math.floor(days / 30.44);
  if (months < 24) return `${months} bulan lalu`;
  return `${Math.floor(months / 12)} tahun lalu`;
}
