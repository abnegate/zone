import { Suspense } from 'react';
import { Outlet } from 'react-router-dom';
import { DownloadDock, TrainDock } from '../../../features/models';
import Sidebar from '../Sidebar/Sidebar';
import './Layout.css';

export default function Layout() {
  return (
    <div className="layout">
      <Sidebar dock={<TrainDock />} />
      <main className="main-content">
        <Suspense
          fallback={
            <div className="loading-container">
              <span className="spinner" />
              <span>Loading...</span>
            </div>
          }
        >
          <Outlet />
        </Suspense>
      </main>
      <div className="layout-docks">
        <DownloadDock />
      </div>
    </div>
  );
}
