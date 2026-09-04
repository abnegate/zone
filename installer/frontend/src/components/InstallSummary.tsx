import {
  type ColumnDef,
  flexRender,
  type StockFeatures,
  stockFeatures,
  useTable,
} from '@tanstack/react-table';
import { formatDistanceToNow } from 'date-fns';
import { useMemo } from 'react';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from './index';

export type InstallSummaryRow = {
  label: string;
  value: string;
};

type InstallSummaryProps = {
  rows: InstallSummaryRow[];
  completedAt: Date;
};

export function InstallSummary({ rows, completedAt }: InstallSummaryProps) {
  const columns = useMemo<ColumnDef<StockFeatures, InstallSummaryRow>[]>(
    () => [
      {
        header: 'Setting',
        accessorKey: 'label',
      },
      {
        header: 'Value',
        accessorKey: 'value',
      },
    ],
    []
  );

  const table = useTable({
    features: stockFeatures,
    data: rows,
    columns,
  });

  return (
    <div className="space-y-3">
      <div className="text-sm text-muted-foreground">
        Completed {formatDistanceToNow(completedAt, { addSuffix: true })}
      </div>
      <Table>
        <TableHeader>
          {table.getHeaderGroups().map((headerGroup) => (
            <TableRow key={headerGroup.id}>
              {headerGroup.headers.map((header) => (
                <TableHead key={header.id}>
                  {header.isPlaceholder
                    ? null
                    : flexRender(header.column.columnDef.header, header.getContext())}
                </TableHead>
              ))}
            </TableRow>
          ))}
        </TableHeader>
        <TableBody>
          {table.getRowModel().rows.map((row) => (
            <TableRow key={row.id}>
              {row.getVisibleCells().map((cell) => (
                <TableCell key={cell.id}>
                  {flexRender(cell.column.columnDef.cell, cell.getContext())}
                </TableCell>
              ))}
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}
