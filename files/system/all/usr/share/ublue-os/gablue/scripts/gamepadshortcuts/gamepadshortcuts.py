import pygame
import os

# Initialiser Pygame
pygame.init()

# Désactiver la prévention de mise en veille
pygame.display.set_allow_screensaver(1)

# Configurer le nombre de manettes
pygame.joystick.init()

# Enlever la sortie sonore de pygame et autres
pygame.mixer.quit()
pygame.font.quit()

# Créer un horloge
clock = pygame.time.Clock()

# Boucle principale
try:

    while True:
        # Attendre un événement
        event = pygame.event.wait() # Bloque jusqu'à ce qu'un événement soit disponible

        if event.type != pygame.JOYBUTTONDOWN and event.type != pygame.JOYDEVICEADDED and event.type != pygame.JOYDEVICEREMOVED:
                continue # Recommencer la boucle si ce n'est pas les events recherchés

        elif event.type == pygame.JOYDEVICEADDED:
                print("Manette connectée.")
                joystick = pygame.joystick.Joystick(0)
                joystick.init()
                # Activer la prévention de mise en veille
                pygame.display.set_allow_screensaver(0)


        elif event.type == pygame.JOYDEVICEREMOVED:
                print("Manette déconnectée.")
                joystick.quit()
                # Désactiver la prévention de mise en veille
                pygame.display.set_allow_screensaver(1)


        elif event.type == pygame.JOYBUTTONDOWN:
                # Vérifier les boutons
                home_button = joystick.get_button(5) # Remplacez par l'index correct pour le bouton Home
                select_button = joystick.get_button(4) # Remplacez par l'index correct pour le bouton Select
                start_button = joystick.get_button(6) # Remplacez par l'index correct pour le bouton Start
                triangle_button= joystick.get_button(3)
                square_button= joystick.get_button(2)
                circle_button= joystick.get_button(1)
                left_button= joystick.get_button(13)
                right_button= joystick.get_button(14)
                down_button= joystick.get_button(12)
                l3_button= joystick.get_button(7)
                r3_button= joystick.get_button(8)

                # Vérifier les combinaisons de boutons
                if home_button and select_button:
                    print("KILL")
                    os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/killthemall &")
                elif home_button and start_button:
                    print("ES")
                    os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/openes &")
                elif home_button and down_button:
                    print("MANGO")
                    os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/showhidemango &")
                elif home_button and left_button:
                    print("SCREEN")
                    os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/takescreenshot &")
                elif home_button and right_button:
                    print("RECORD")
                    os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/startstoprecord &")
                    pygame.time.wait(2000)
                elif home_button and l3_button:
                    print("CLIC")
                    os.system("ffplay -nodisp -autoexit /usr/share/ublue-os/gablue/scripts/gamepadshortcuts/gun1.wav &")
                elif home_button and r3_button:
                    print("PAN")
                    os.system("ffplay -nodisp -autoexit /usr/share/ublue-os/gablue/scripts/gamepadshortcuts/gun2.wav &")

        # Limiter la fréquence des événements
        clock.tick(30)

except KeyboardInterrupt:
    print("Arrêt du script.")
finally:
    pygame.quit()
