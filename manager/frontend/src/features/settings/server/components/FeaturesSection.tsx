import { FeaturesPanel, useModels, usePull } from '../../../models';

export default function FeaturesSection() {
  const pull = usePull();
  const { refresh } = useModels();
  return <FeaturesPanel pull={pull} onInstalled={refresh} />;
}
