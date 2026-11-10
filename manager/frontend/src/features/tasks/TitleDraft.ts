export class TitleDraft {
  value: string;
  start = 0;
  end: number;

  constructor(readonly original: string) {
    this.value = original;
    this.end = original.length;
  }
}
