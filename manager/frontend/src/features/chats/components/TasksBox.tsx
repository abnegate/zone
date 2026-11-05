import type { ChatTasks } from '../hooks/useChatTasks';
import { panelKey } from '../utils/panel';
import { PanelBox } from './PanelBox';
import { TaskItem } from './TaskItem';
import './TasksBox.css';

export interface TasksBoxProps {
  userId: string | null;
  tasks: ChatTasks;
}

export function TasksBox({ userId, tasks }: TasksBoxProps) {
  return (
    <PanelBox
      title="Tasks"
      count={tasks.tasks.length}
      storageKey={panelKey(userId, 'tasks')}
      className="chat-tasks"
    >
      <ul className="chat-task-list">
        {tasks.tasks.map((task) => (
          <TaskItem key={task.id} task={task} />
        ))}
      </ul>
      {tasks.error && (
        <p className="chat-tasks-error" role="alert">
          {tasks.error}
        </p>
      )}
    </PanelBox>
  );
}
