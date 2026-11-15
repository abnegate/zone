export function firstLine(message: string): string {
  const end = message.indexOf('\n');
  return (end === -1 ? message : message.slice(0, end)).trim();
}
