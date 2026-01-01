import type { IMAPConfig } from '../types';
import { MailIcon } from './icons';
import type { SourceDefinition } from './index';

export const imapSource: SourceDefinition = {
  id: 'imap',
  name: 'Email',
  category: 'mail',
  description: 'Connect to an email inbox',
  icon: MailIcon,
  badgeColor: 'badge-yellow',
  iconWrapperClass: 'imap',
  enabled: true,

  formFields: [
    {
      fields: [
        {
          id: 'imapHost',
          label: 'IMAP Server',
          type: 'text',
          placeholder: 'imap.gmail.com',
          required: true,
        },
        { id: 'imapPort', label: 'Port', type: 'number', placeholder: '993', defaultValue: 993 },
      ],
    },
    {
      id: 'imapUsername',
      label: 'Username',
      type: 'text',
      placeholder: 'user@example.com',
      required: true,
    },
    {
      fields: [
        {
          id: 'imapFolder',
          label: 'Folder',
          type: 'text',
          placeholder: 'INBOX',
          defaultValue: 'INBOX',
        },
        {
          id: 'imapUseSsl',
          label: 'SSL',
          type: 'toggle',
          defaultValue: true,
          toggleTitle: 'Use SSL/TLS',
        },
      ],
    },
  ],

  credentialField: {
    id: 'credentials',
    label: 'Password',
    type: 'password',
    placeholder: 'App password or regular password',
    required: true,
    hint: 'For Gmail, use an App Password',
  },

  buildConfig: (state): IMAPConfig => ({
    host: state.imapHost as string,
    port: state.imapPort as number,
    username: state.imapUsername as string,
    use_ssl: state.imapUseSsl as boolean,
    folder: (state.imapFolder as string) || 'INBOX',
  }),

  getDefaultName: (state) => `${state.imapUsername}@${state.imapHost}`,

  getFieldIds: () => ['imapHost', 'imapPort', 'imapUsername', 'imapUseSsl', 'imapFolder'],
};
