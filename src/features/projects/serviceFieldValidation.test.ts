import { describe, expect, it } from 'vitest';
import {
  MAX_SERVICE_COMMAND_LENGTH,
  MAX_SERVICE_NAME_LENGTH,
  validateServiceCommand,
  validateServiceName,
  validateServiceWorkingDirectory,
} from './serviceFieldValidation';

describe('validateServiceName', () => {
  it('requires a non-empty trimmed name', () => {
    expect(validateServiceName('   ')).toBe('Service name is required.');
  });

  it('accepts a valid name and the exact length limit', () => {
    expect(validateServiceName('API service')).toBeNull();
    expect(validateServiceName('a'.repeat(MAX_SERVICE_NAME_LENGTH))).toBeNull();
  });

  it('rejects a name beyond the length limit', () => {
    expect(validateServiceName('a'.repeat(MAX_SERVICE_NAME_LENGTH + 1))).toBe(
      'Use at most 120 characters.',
    );
  });
});

describe('validateServiceCommand', () => {
  it('requires a non-empty trimmed command', () => {
    expect(validateServiceCommand(' \t ')).toBe('Command is required.');
  });

  it('accepts a valid command and the exact length limit', () => {
    expect(validateServiceCommand('npm run -- dev')).toBeNull();
    expect(validateServiceCommand('a'.repeat(MAX_SERVICE_COMMAND_LENGTH))).toBeNull();
  });

  it('rejects a command beyond the length limit', () => {
    expect(validateServiceCommand('a'.repeat(MAX_SERVICE_COMMAND_LENGTH + 1))).toBe(
      'Use at most 4096 characters.',
    );
  });

  it.each([
    ['NUL', 'echo\0marker'],
    ['CR', 'echo\rmarker'],
    ['LF', 'echo\nmarker'],
  ])('rejects %s characters', (_label, command) => {
    expect(validateServiceCommand(command)).toBe('NUL characters and line breaks are not allowed.');
  });
});

describe('validateServiceWorkingDirectory', () => {
  it('requires a non-empty trimmed directory', () => {
    expect(validateServiceWorkingDirectory('  ')).toBe(
      "Working directory is required. Use '.' for the root.",
    );
  });

  it.each(['.', 'client', 'apps/web', 'apps\\web'])(
    'accepts the relative directory %s',
    (directory) => {
      expect(validateServiceWorkingDirectory(directory)).toBeNull();
    },
  );

  it.each(['C:\\project', 'c:project', '\\project', '/project', '\\\\server\\share'])(
    'rejects the absolute or rooted directory %s',
    (directory) => {
      expect(validateServiceWorkingDirectory(directory)).toBe(
        'Use a path relative to the project root.',
      );
    },
  );

  it.each(['../server', 'client/../server', 'client\\..\\server'])(
    'rejects a parent component in %s',
    (directory) => {
      expect(validateServiceWorkingDirectory(directory)).toBe(
        "Parent components ('..') are not allowed.",
      );
    },
  );

  it('does not reject dots that are part of a normal component', () => {
    expect(validateServiceWorkingDirectory('client..name/subdir')).toBeNull();
  });
});
