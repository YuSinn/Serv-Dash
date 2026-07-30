import { useEffect, useRef, useState } from 'react';
import type { ServiceDefinition, ServiceLogsSnapshot, ServiceRuntime } from './types';

interface ServiceLogsPanelProps {
  service: ServiceDefinition | null;
  runtime: ServiceRuntime | null;
  logs: ServiceLogsSnapshot | null;
  isBusy: boolean;
  onClear: () => void;
}

function formatLogTime(timestamp: string): string {
  const date = new Date(timestamp);
  return Number.isNaN(date.getTime()) ? timestamp : date.toLocaleTimeString([], { hour12: false });
}

export function ServiceLogsPanel({
  service,
  runtime,
  logs,
  isBusy,
  onClear,
}: ServiceLogsPanelProps) {
  const viewport = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  const entries = logs?.entries ?? [];

  useEffect(() => {
    setAutoScroll(true);
  }, [service?.id]);

  useEffect(() => {
    const element = viewport.current;
    if (element && autoScroll) {
      element.scrollTop = element.scrollHeight;
    }
  }, [autoScroll, entries.length]);

  function trackScroll() {
    const element = viewport.current;
    if (!element) {
      return;
    }
    const distanceFromBottom = element.scrollHeight - element.scrollTop - element.clientHeight;
    setAutoScroll(distanceFromBottom < 32);
  }

  return (
    <section className="service-logs-panel" aria-labelledby="service-logs-title">
      <header className="logs-panel-header">
        <div>
          <p className="eyebrow">Live process output</p>
          <h2 id="service-logs-title">{service ? `${service.name} logs` : 'Service logs'}</h2>
          <p>
            {service
              ? `${entries.length} buffered entries${runtime ? ` | ${runtime.status}` : ''}`
              : 'Select a service to inspect stdout and stderr.'}
          </p>
        </div>
        <div className="logs-panel-actions">
          <label className="auto-scroll-toggle">
            <input
              type="checkbox"
              checked={autoScroll}
              disabled={!service}
              onChange={(event) => setAutoScroll(event.target.checked)}
            />
            Auto-scroll
          </label>
          <button
            className="secondary-button"
            type="button"
            disabled={!service || isBusy || entries.length === 0}
            onClick={onClear}
          >
            Clear logs
          </button>
        </div>
      </header>

      <div ref={viewport} className="logs-viewport" tabIndex={0} onScroll={trackScroll}>
        {!service ? (
          <div className="logs-empty-state">
            Choose <strong>View logs</strong> on a service card. Output is kept in memory only.
          </div>
        ) : logs === null ? (
          <div className="logs-empty-state">Loading buffered output...</div>
        ) : entries.length === 0 ? (
          <div className="logs-empty-state">
            No output for this service. Start it to capture stdout and stderr.
          </div>
        ) : (
          <ol className="log-entries" aria-label={`${service.name} process output`}>
            {entries.map((entry) => (
              <li key={entry.sequence} className={`log-entry log-${entry.source}`}>
                <span className="log-sequence">{entry.sequence}</span>
                <time dateTime={entry.timestamp}>{formatLogTime(entry.timestamp)}</time>
                <span className="log-source">{entry.source}</span>
                <span className="log-text">{entry.text || ' '}</span>
              </li>
            ))}
          </ol>
        )}
      </div>
      <footer className="logs-panel-footer">
        Maximum 2,000 entries and approximately 2 MiB per service. Oldest entries are discarded
        first; long lines are split at 16 KiB.
      </footer>
    </section>
  );
}
