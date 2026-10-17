/** Splits seconds into whole hours and the remaining minutes. */
export function splitPlaytime(seconds: number): { hours: number; minutes: number } {
  const totalMinutes = Math.floor(Math.max(0, seconds) / 60);
  return { hours: Math.floor(totalMinutes / 60), minutes: totalMinutes % 60 };
}
