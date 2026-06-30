#!/usr/bin/python3

import evdev
import select
import subprocess
import sys
import time
from PySide6.QtWidgets import QApplication, QLabel, QWidget, QVBoxLayout, QGraphicsDropShadowEffect, QFrame
from PySide6.QtCore import QTimer, Qt
from PySide6.QtGui import QFont, QColor
from evdev import ecodes

# Options du menu
OPTIONS = ["Déconnecter Bluetooth", "Mettre en veille", "Éteindre", "Redémarrer"]

# Mapping boutons
BTN_CROSS = ecodes.BTN_A
BTN_CIRCLE = ecodes.BTN_B

class MenuVR(QWidget):
    def __init__(self):
        super().__init__()
        
        self.selected_option = 0
        self.confirm_mode = False
        self.confirm_action = None
        self.last_input_time = time.time() * 1000
        self.inactivity_timeout = 5000
        self.cooldown = 200
        self.last_up = False
        self.last_down = False
        self.last_cross = False
        self.last_circle = False
        self.gamepad = None
        
        self.init_ui()
        self.find_gamepad()
        
        self.timer = QTimer(self)
        self.timer.timeout.connect(self.update_loop)
        self.timer.start(16)
        
        self.inactivity_timer = QTimer(self)
        self.inactivity_timer.timeout.connect(self.check_inactivity)
        self.inactivity_timer.start(100)
    
    def init_ui(self):
        self.setWindowFlags(Qt.FramelessWindowHint | Qt.WindowStaysOnTopHint | Qt.Tool)
        self.setAttribute(Qt.WA_TranslucentBackground)
        self.setFixedSize(420, 250)
        
        self.container = QFrame(self)
        self.container.setGeometry(0, 0, 420, 250)
        self.container.setStyleSheet("""
            QFrame {
                background-color: rgba(0, 0, 0, 77);
                border-radius: 20px;
            }
        """)
        
        shadow = QGraphicsDropShadowEffect(self.container)
        shadow.setBlurRadius(30)
        shadow.setColor(QColor(0, 0, 0, 180))
        shadow.setOffset(0, 10)
        self.container.setGraphicsEffect(shadow)
        
        self.layout = QVBoxLayout(self.container)
        self.layout.setAlignment(Qt.AlignCenter)
        self.layout.setContentsMargins(30, 20, 30, 20)
        self.layout.setSpacing(25)
        
        self.label = QLabel()
        self.label.setAlignment(Qt.AlignCenter)
        self.label.setFont(QFont("Arial", 20))
        self.label.setWordWrap(True)
        self.label.setStyleSheet("background: transparent; color: rgba(255, 255, 255, 230);")
        
        self.layout.addWidget(self.label)
        
        self.update_display()
        self.show()
    
    def find_gamepad(self):
        devices = [evdev.InputDevice(path) for path in evdev.list_devices()]
        for device in devices:
            capabilities = device.capabilities()
            if ecodes.EV_KEY in capabilities:
                keys = capabilities[ecodes.EV_KEY]
                if ecodes.BTN_A in keys or ecodes.BTN_SELECT in keys:
                    self.gamepad = device
                    print(f"Manette trouvée: {device.name} ({device.path})")
                    return
        print("Aucune manette détectée")
    
    def update_display(self):
        if self.confirm_mode:
            self.label.setText(f"Confirmer {self.confirm_action} ?\n\n<span style='color: rgba(50, 200, 255, 200);'>✓ Croix: Oui</span>   <span style='color: rgba(255, 100, 100, 200);'>✗ Rond: Non</span>")
        else:
            lines = []
            for i, option in enumerate(OPTIONS):
                if i == self.selected_option:
                    lines.append(f'<p style="margin: 0; padding: 0; line-height: 1.8;"><span style="color: rgb(80, 200, 255);">{option}</span></p>')
                else:
                    lines.append(f'<p style="margin: 0; padding: 0; line-height: 1.8;"><span style="color: rgba(255, 255, 255, 180);">{option}</span></p>')
            self.label.setText("".join(lines))
    
    def check_inactivity(self):
        current_time = time.time() * 1000
        if current_time - self.last_input_time > self.inactivity_timeout:
            print("Inactivité détectée, fermeture du menu")
            self.close_app()
    
    def close_app(self):
        self.timer.stop()
        self.inactivity_timer.stop()
        if self.gamepad:
            self.gamepad.close()
        QApplication.quit()
    
    def hide_and_execute(self, action, args=None):
        self.hide()
        self.timer.stop()
        self.inactivity_timer.stop()
        QTimer.singleShot(50, lambda: self.execute_action(action, args))
    
    def execute_action(self, action, args=None):
        if action == "bluetooth":
            subprocess.run(["/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/decoblue"])
        elif action == "suspend":
            subprocess.run(["systemctl", "suspend"])
        elif action == "poweroff":
            subprocess.run(["systemctl", "poweroff"])
        elif action == "reboot":
            subprocess.run(["systemctl", "reboot"])
        QApplication.quit()
    
    def update_loop(self):
        if not self.gamepad:
            self.find_gamepad()
            return
        
        try:
            r, w, x = select.select([self.gamepad], [], [], 0)
        except (OSError, IOError):
            print("Manette déconnectée")
            self.gamepad = None
            return
        
        if self.gamepad not in r:
            return
        
        current_time = time.time() * 1000
        
        if current_time - self.last_input_time < self.cooldown:
            return
        
        try:
            hat_x = 0
            hat_y = 0
            axis_y = 0.0
            cross_pressed = False
            circle_pressed = False
            
            for event in self.gamepad.read():
                if event.type == ecodes.EV_ABS:
                    if event.code == ecodes.ABS_HAT0X:
                        hat_x = event.value
                    elif event.code == ecodes.ABS_HAT0Y:
                        hat_y = event.value
                    elif event.code == ecodes.ABS_Y:
                        axis_y = (event.value - 128) / 127.0
                elif event.type == ecodes.EV_KEY:
                    if event.code == BTN_CROSS:
                        cross_pressed = event.value == 1
                    elif event.code == BTN_CIRCLE:
                        circle_pressed = event.value == 1
            
            up = hat_y == 1 or axis_y < -0.5
            down = hat_y == -1 or axis_y > 0.5
            
            if self.confirm_mode:
                if cross_pressed and not self.last_cross:
                    print(f"Confirmation de l'action: {self.confirm_action}")
                    action = "poweroff" if self.confirm_action == "Éteindre" else "reboot"
                    self.hide_and_execute(action)
                    return
                elif circle_pressed and not self.last_circle:
                    print("Annulation de la confirmation")
                    self.confirm_mode = False
                    self.confirm_action = None
                    self.update_display()
                    self.last_input_time = current_time
            else:
                if up and not self.last_up:
                    self.selected_option = (self.selected_option - 1) % len(OPTIONS)
                    self.update_display()
                    self.last_input_time = current_time
                    print(f"Option sélectionnée: {OPTIONS[self.selected_option]}")
                elif down and not self.last_down:
                    self.selected_option = (self.selected_option + 1) % len(OPTIONS)
                    self.update_display()
                    self.last_input_time = current_time
                    print(f"Option sélectionnée: {OPTIONS[self.selected_option]}")
                
                if cross_pressed and not self.last_cross:
                    print(f"Action choisie: {OPTIONS[self.selected_option]}")
                    self.last_input_time = current_time
                    
                    if self.selected_option == 0:
                        self.hide_and_execute("bluetooth")
                        return
                    elif self.selected_option == 1:
                        self.hide_and_execute("suspend")
                        return
                    elif self.selected_option == 2:
                        self.confirm_mode = True
                        self.confirm_action = "Éteindre"
                        self.update_display()
                    elif self.selected_option == 3:
                        self.confirm_mode = True
                        self.confirm_action = "Redémarrer"
                        self.update_display()
                
                if circle_pressed and not self.last_circle:
                    print("Annulation")
                    self.close_app()
                    return
            
            self.last_up = up
            self.last_down = down
            self.last_cross = cross_pressed
            self.last_circle = circle_pressed
            
        except (OSError, IOError, BlockingIOError):
            print("Erreur lecture manette")
            self.gamepad = None

def main():
    app = QApplication(sys.argv)
    app.setQuitOnLastWindowClosed(True)
    
    menu = MenuVR()
    
    sys.exit(app.exec())

if __name__ == "__main__":
    main()
