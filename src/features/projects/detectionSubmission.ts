import {
  validateDetectionDraft,
  type DetectionDraft,
  type DetectionDraftErrors,
} from './detectionDraft';
import type { DetectedServiceSubmission, ServiceDefinition, ServiceInput } from './types';

export type DetectionSubmissionSkipKind =
  | 'alreadyRegisteredMatch'
  | 'invalidDraft'
  | 'duplicateExistingName'
  | 'duplicateExistingWorkingDirectoryCommand'
  | 'duplicateSelectionName'
  | 'duplicateSelectionWorkingDirectoryCommand';

export interface PreparedDetectedService {
  readonly stableId: string;
  readonly input: ServiceInput;
}

interface SkippedDetectedServiceBase {
  readonly stableId: string;
  readonly displayName: string;
}

export type SkippedDetectedService =
  | (SkippedDetectedServiceBase & {
      readonly kind: 'invalidDraft';
      readonly errors: DetectionDraftErrors;
    })
  | (SkippedDetectedServiceBase & {
      readonly kind: Exclude<DetectionSubmissionSkipKind, 'invalidDraft'>;
    });

export interface DetectionSubmissionPlan {
  readonly eligible: readonly PreparedDetectedService[];
  readonly skipped: readonly SkippedDetectedService[];
  readonly missingSelectedIds: readonly string[];
}

export function preparedDetectedServicesToSubmissions(
  prepared: readonly PreparedDetectedService[],
): DetectedServiceSubmission[] {
  return prepared.map(({ stableId, input }) => ({ stableId, service: input }));
}

export function draftToServiceInput(draft: DetectionDraft): ServiceInput {
  return {
    name: draft.displayName.trim(),
    workingDirectory: draft.workingDirectory.trim(),
    command: draft.command.trim(),
    expectedPort: null,
    localUrl: null,
  };
}

function caseInsensitiveKey(value: string): string {
  return value.trim().toLowerCase();
}

function functionalKey(workingDirectory: string, command: string): string {
  return JSON.stringify([caseInsensitiveKey(workingDirectory), command.trim()]);
}

export function buildDetectionSubmissionPlan({
  drafts,
  selectedIds,
  registeredServices,
}: {
  readonly drafts: readonly DetectionDraft[];
  readonly selectedIds: ReadonlySet<string>;
  readonly registeredServices: readonly ServiceDefinition[];
}): DetectionSubmissionPlan {
  const registeredNames = new Set(
    registeredServices.map((service) => caseInsensitiveKey(service.name)),
  );
  const registeredFunctions = new Set(
    registeredServices.map((service) => functionalKey(service.workingDirectory, service.command)),
  );
  const eligibleNames = new Set<string>();
  const eligibleFunctions = new Set<string>();
  const eligible: PreparedDetectedService[] = [];
  const skipped: SkippedDetectedService[] = [];

  for (const draft of drafts) {
    if (!selectedIds.has(draft.stableId)) {
      continue;
    }

    const skippedBase = { stableId: draft.stableId, displayName: draft.displayName };
    if (draft.matchesRegisteredService) {
      skipped.push({ ...skippedBase, kind: 'alreadyRegisteredMatch' });
      continue;
    }

    const errors = validateDetectionDraft(draft);
    if (Object.keys(errors).length > 0) {
      skipped.push({ ...skippedBase, kind: 'invalidDraft', errors });
      continue;
    }

    const input = draftToServiceInput(draft);
    const nameKey = caseInsensitiveKey(input.name);
    const serviceKey = functionalKey(input.workingDirectory, input.command);

    if (registeredNames.has(nameKey)) {
      skipped.push({ ...skippedBase, kind: 'duplicateExistingName' });
    } else if (registeredFunctions.has(serviceKey)) {
      skipped.push({
        ...skippedBase,
        kind: 'duplicateExistingWorkingDirectoryCommand',
      });
    } else if (eligibleNames.has(nameKey)) {
      skipped.push({ ...skippedBase, kind: 'duplicateSelectionName' });
    } else if (eligibleFunctions.has(serviceKey)) {
      skipped.push({
        ...skippedBase,
        kind: 'duplicateSelectionWorkingDirectoryCommand',
      });
    } else {
      eligible.push({ stableId: draft.stableId, input });
      eligibleNames.add(nameKey);
      eligibleFunctions.add(serviceKey);
    }
  }

  const draftIds = new Set(drafts.map(({ stableId }) => stableId));
  const missingSelectedIds = [...selectedIds].filter((stableId) => !draftIds.has(stableId)).sort();

  return { eligible, skipped, missingSelectedIds };
}
