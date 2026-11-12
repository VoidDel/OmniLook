import { describe, expect, it } from 'vitest';

import { stepName } from './occt';

describe('occt', () => {
  it('writes out STEP name escapes', () => {
    expect(stepName('8p-\\X2\\8033673A\\X0\\')).toBe('8p-耳机');
    expect(stepName('8p-_X2_8033673a_X0_')).toBe('8p-耳机');
    expect(stepName('caf\\X\\E9')).toBe('café');
    expect(stepName('plain_X2_name')).toBe('plain_X2_name');
  });
});
