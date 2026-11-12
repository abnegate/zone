export class Selection {
  static readonly EMPTY = new Selection();

  readonly ids: ReadonlySet<string>;

  constructor(
    ids: Iterable<string> = [],
    readonly anchor: string | null = null
  ) {
    this.ids = new Set(ids);
  }

  get size(): number {
    return this.ids.size;
  }

  has(id: string): boolean {
    return this.ids.has(id);
  }

  toggle(id: string): Selection {
    const ids = new Set(this.ids);
    if (!ids.delete(id)) {
      ids.add(id);
    }
    return new Selection(ids, id);
  }

  extend(id: string, column: readonly string[]): Selection {
    const start = this.anchor === null ? -1 : column.indexOf(this.anchor);
    const end = column.indexOf(id);
    if (start < 0 || end < 0) {
      return new Selection([...this.ids, id], id);
    }
    const range = column.slice(Math.min(start, end), Math.max(start, end) + 1);
    return new Selection([...this.ids, ...range], this.anchor);
  }

  retain(listed: Iterable<string>): Selection {
    const available = new Set(listed);
    const ids = [...this.ids].filter((id) => available.has(id));
    const anchor = this.anchor !== null && available.has(this.anchor) ? this.anchor : null;
    if (ids.length === this.ids.size && anchor === this.anchor) {
      return this;
    }
    return new Selection(ids, anchor);
  }

  clear(): Selection {
    return Selection.EMPTY;
  }
}
