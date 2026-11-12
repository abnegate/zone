export class Selection {
  static readonly EMPTY = new Selection();

  readonly ids: ReadonlySet<string>;

  private readonly base: ReadonlySet<string>;

  constructor(
    ids: Iterable<string> = [],
    readonly anchor: string | null = null,
    base?: Iterable<string>
  ) {
    this.ids = new Set(ids);
    this.base = base === undefined ? this.ids : new Set(base);
  }

  get size(): number {
    return this.ids.size;
  }

  has(id: string): boolean {
    return this.ids.has(id);
  }

  toggle(id: string): Selection {
    const ids = new Set(this.ids);
    if (ids.delete(id)) {
      return new Selection(ids);
    }
    ids.add(id);
    return new Selection(ids, id);
  }

  extend(id: string, column: readonly string[]): Selection {
    const start = this.anchor === null ? -1 : column.indexOf(this.anchor);
    const end = column.indexOf(id);
    if (start < 0 || end < 0) {
      return new Selection([...this.ids, id], id);
    }
    const range = column.slice(Math.min(start, end), Math.max(start, end) + 1);
    return new Selection([...this.base, ...range], this.anchor, this.base);
  }

  retain(listed: Iterable<string>): Selection {
    const available = new Set(listed);
    const ids = [...this.ids].filter((id) => available.has(id));
    const base = [...this.base].filter((id) => available.has(id));
    const anchor = this.anchor !== null && available.has(this.anchor) ? this.anchor : null;
    if (ids.length === this.ids.size && base.length === this.base.size && anchor === this.anchor) {
      return this;
    }
    return new Selection(ids, anchor, base);
  }
}
