import { Outlet } from 'react-router-dom';
import { DownloadDock, TrainDock } from '../../../features/models';
import Sidebar from '../Sidebar/Sidebar';
import './Layout.css';

export default function Layout() {
  return (
    <div className="layout">
      <Sidebar />
      <main className="main-content">
        <Outlet />
      </main>
      <div className="layout-docks">
        <DownloadDock />
        <TrainDock />
      </div>
    </div>
  );
}
