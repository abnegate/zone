import { ApiError } from '../../../api/ApiError';
import type { AuditCode, AuditRefusalBody } from './types';

export class AuditRefusal extends ApiError {
  readonly code: AuditCode;

  constructor(refusal: AuditRefusalBody, status: number) {
    super(refusal.error, status);
    this.name = 'AuditRefusal';
    this.code = refusal.code;
  }
}
