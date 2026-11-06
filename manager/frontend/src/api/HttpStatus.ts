export const HttpStatus = {
  NotFound: 404,
} as const;

export type HttpStatus = (typeof HttpStatus)[keyof typeof HttpStatus];
