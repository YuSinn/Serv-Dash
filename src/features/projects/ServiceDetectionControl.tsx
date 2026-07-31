import { useCallback, useRef } from 'react';
import { ServiceDetectionDialog } from './ServiceDetectionDialog';
import './serviceDetection.css';
import { useServiceDetection } from './useServiceDetection';

interface ServiceDetectionControlProps {
  projectId: string;
}

export function ServiceDetectionControl({ projectId }: ServiceDetectionControlProps) {
  const detection = useServiceDetection(projectId);
  const triggerRef = useRef<HTMLButtonElement>(null);

  const handleDetect = useCallback(() => {
    detection.open();
    void detection.scan();
  }, [detection.open, detection.scan]);

  const handleClose = useCallback(() => {
    detection.close();
    triggerRef.current?.focus();
  }, [detection.close]);

  return (
    <>
      <button
        ref={triggerRef}
        className="secondary-button"
        type="button"
        disabled={detection.isLoading}
        aria-busy={detection.isLoading}
        onClick={handleDetect}
      >
        {detection.isLoading ? 'Detecting...' : 'Detect services'}
      </button>
      {detection.isOpen ? (
        <ServiceDetectionDialog detection={detection} onClose={handleClose} />
      ) : null}
    </>
  );
}
