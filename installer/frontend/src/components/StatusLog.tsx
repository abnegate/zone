import React, { useEffect, useRef } from 'react';

interface StatusLine {
  message: string;
  type?: 'normal' | 'success' | 'error';
}

interface StatusLogProps {
  lines: StatusLine[];
}

export function StatusLog({ lines }: StatusLogProps) {
  const logRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [lines]);

  return (
    <div className="status-log" ref={logRef}>
      {lines.map((line, index) => (
        <div key={index} className={`status-line ${line.type || ''}`}>
          {line.message}
        </div>
      ))}
    </div>
  );
}
