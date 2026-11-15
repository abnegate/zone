import { useNavDrawer } from '../../context/NavDrawerContext';

export type BarBack = {
  label: string;
  onClick: () => void;
};

/// The phone app bar's leading slot. A screen reached from a list leads with
/// Back; every other screen leads with the drawer, so the two never stack.
/// The slot is laid out away on a desktop, where the sidebar is always there.
export default function BarLead({ back }: { back?: BarBack }) {
  const drawer = useNavDrawer();

  if (back) {
    return (
      <div className="page-bar-lead">
        <button
          type="button"
          className="page-bar-lead-btn"
          onClick={back.onClick}
          aria-label={back.label}
          title={back.label}
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <path d="M15 19l-7-7 7-7" />
          </svg>
        </button>
      </div>
    );
  }

  if (!drawer) return null;

  return (
    <div className="page-bar-lead">
      <button
        type="button"
        className="page-bar-lead-btn"
        onClick={drawer.toggle}
        aria-label="Toggle menu"
        aria-expanded={drawer.open}
        title="Menu"
      >
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          aria-hidden="true"
        >
          <path d="M4 6h16M4 12h16M4 18h16" />
        </svg>
      </button>
    </div>
  );
}
