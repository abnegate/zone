SET LOCAL lock_timeout = '5s';

CREATE TABLE public.audit_deliveries (
    destination_id uuid NOT NULL,
    transaction_id bigint NOT NULL,
    sequence bigint NOT NULL,
    batch_transaction_id bigint,
    batch_sequence bigint,
    attempts integer DEFAULT 0 NOT NULL,
    next_attempt_at timestamp with time zone DEFAULT now() NOT NULL,
    lease_owner uuid,
    lease_expires_at timestamp with time zone,
    last_success_at timestamp with time zone,
    last_failure_at timestamp with time zone,
    last_error_code text,
    last_error text,
    delivered_count bigint DEFAULT 0 NOT NULL,
    CONSTRAINT audit_deliveries_pkey PRIMARY KEY (destination_id),
    CONSTRAINT audit_deliveries_batch_check CHECK ((batch_transaction_id IS NULL) = (batch_sequence IS NULL)),
    CONSTRAINT audit_deliveries_destination_id_fkey FOREIGN KEY (destination_id) REFERENCES public.audit_destinations(id) ON DELETE CASCADE
);
