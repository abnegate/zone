export const HttpStatus = {
  BadRequest: 400,
  Forbidden: 403,
  NotFound: 404,
  Conflict: 409,
  InternalServerError: 500,
  BadGateway: 502,
} as const;

export type HttpStatus = (typeof HttpStatus)[keyof typeof HttpStatus];
