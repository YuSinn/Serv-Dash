export const MAX_SERVICE_NAME_LENGTH = 120;
export const MAX_SERVICE_COMMAND_LENGTH = 4096;

export function validateServiceName(value: string): string | null {
  const name = value.trim();
  if (!name) {
    return 'Service name is required.';
  }
  if (name.length > MAX_SERVICE_NAME_LENGTH) {
    return 'Use at most 120 characters.';
  }
  return null;
}

export function validateServiceCommand(value: string): string | null {
  const command = value.trim();
  if (!command) {
    return 'Command is required.';
  }
  if (command.length > MAX_SERVICE_COMMAND_LENGTH) {
    return 'Use at most 4096 characters.';
  }
  if (/\0|[\r\n]/.test(command)) {
    return 'NUL characters and line breaks are not allowed.';
  }
  return null;
}

export function validateServiceWorkingDirectory(value: string): string | null {
  const directory = value.trim();
  if (!directory) {
    return "Working directory is required. Use '.' for the root.";
  }
  if (/^(?:[a-z]:|[\\/])/i.test(directory)) {
    return 'Use a path relative to the project root.';
  }
  if (directory.split(/[\\/]+/).includes('..')) {
    return "Parent components ('..') are not allowed.";
  }
  return null;
}
